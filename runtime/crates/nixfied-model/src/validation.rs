use std::collections::BTreeSet;

use crate::constants::{MODEL_VERSION, TOOLCHAIN_ID, runtime_abi};
use crate::error::ValidationError;
use crate::ids::{OperationId, ServiceId};
use crate::types::*;

pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

impl Validate for Model {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_exact_identities(self)?;
        validate_required_strings(self)?;
        validate_unit_ids(self)?;
        validate_codebases(self)?;
        validate_environments(self)?;
        validate_slot_policy(&self.slot_policy)?;
        validate_slot_placements(self)?;
        validate_invocations(self)?;
        validate_closures(self)?;
        validate_services(self)?;
        validate_tasks(self)?;
        // Cross-reference resolution (execs/closures/operations/workflow/env
        // references) is proven by the runtime's `lower` step, which builds the
        // executor input only from references that exist. `validate` covers the
        // identity, value, and structural contract a single document must hold on
        // its own; admission then lowers it.
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
    if model.runtime_abi != runtime_abi() {
        return Err(ValidationError::RuntimeAbi {
            expected: runtime_abi(),
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

/// Exec/closure/service/task/workflow/node ids name filesystem artifacts (log
/// files, registry keys), so they must be path-safe: no separators, no
/// traversal, no leading dot. Same shape the installer enforces on project ids.
fn validate_unit_ids(model: &Model) -> Result<(), ValidationError> {
    for id in model.closures.keys() {
        require_path_safe_id("closures", id)?;
    }
    for id in model.services.keys() {
        require_step_safe_id("services", id)?;
    }
    for id in model.tasks.keys() {
        require_step_safe_id("tasks", id)?;
    }
    Ok(())
}

fn require_path_safe_id(field: &'static str, id: &str) -> Result<(), ValidationError> {
    let mut chars = id.chars();
    let valid = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedValue {
            field,
            expected: "an id matching [A-Za-z0-9][A-Za-z0-9._-]*",
            actual: id.to_string(),
        })
    }
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        Err(ValidationError::EmptyField { field })
    } else {
        Ok(())
    }
}

/// A single live-workspace codebase named `main`. Multi-codebase source identity
/// is a later milestone; the runtime admission layer still resolves one root.
fn validate_codebases(model: &Model) -> Result<(), ValidationError> {
    if model.codebases.len() != 1 {
        return Err(ValidationError::ExpectedOne { field: "codebases" });
    }
    let codebase = &model.codebases[0];
    expect_string(
        "codebases[0].codebaseId",
        "main",
        codebase.codebase_id.as_str(),
    )?;
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

/// A single `dev` isolation namespace. Multi-environment isolation is a later
/// milestone; membership does not exist at all (a run's services derive from
/// its selected task).
fn validate_environments(model: &Model) -> Result<(), ValidationError> {
    if model.environments.len() != 1
        || model.environments.iter().next().map(String::as_str) != Some("dev")
    {
        return Err(ValidationError::UnsupportedValue {
            field: "environments",
            expected: "[\"dev\"]",
            actual: format!("{:?}", model.environments.as_slice()),
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
        validate_candidate_port_window(
            "placement.slotPlacements.candidatePorts",
            &placement.candidate_ports,
        )?;
        windows.push((
            placement.candidate_ports.start,
            placement.candidate_ports.end,
        ));
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

/// Every inline invocation carries a non-empty argv, a resolved executable,
/// and a confined cwd. Tool/closure reference resolution and the run[0]
/// resolution rule are proven by the runtime's `lower` step.
fn validate_invocations(model: &Model) -> Result<(), ValidationError> {
    for task in model.tasks.values() {
        if let Some(invocation) = &task.invocation {
            validate_invocation(invocation)?;
        }
    }
    for service in model.services.values() {
        for invocation in lifecycle_invocations(&service.lifecycle) {
            validate_invocation(invocation)?;
        }
    }
    Ok(())
}

/// Every invocation a lifecycle carries, in canonical order.
fn lifecycle_invocations(lifecycle: &Lifecycle) -> impl Iterator<Item = &InvocationSpec> {
    lifecycle
        .prepare
        .invocation
        .iter()
        .chain(std::iter::once(&lifecycle.start.invocation))
        .chain(lifecycle.ready.probe.invocation.iter())
        .chain(lifecycle.health.probe.invocation.iter())
}

fn validate_invocation(invocation: &InvocationSpec) -> Result<(), ValidationError> {
    if invocation.run.is_empty() {
        return Err(ValidationError::UnsupportedValue {
            field: "invocation.run",
            expected: "a non-empty argv",
            actual: "[]".to_string(),
        });
    }
    require_non_empty("invocation.run[0]", &invocation.run[0])?;
    require_non_empty("invocation.executable", &invocation.executable)?;
    require_non_empty("invocation.cwd", &invocation.cwd)?;
    if invocation.tools.is_empty() {
        return Err(ValidationError::UnsupportedValue {
            field: "invocation.tools",
            expected: "at least one tool closure",
            actual: "[]".to_string(),
        });
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

/// At least one runnable unit: services may be empty as long as bounded tasks
/// exist (the compiler admits task-only models). Each declared service carries
/// the full generic lifecycle contract.
fn validate_services(model: &Model) -> Result<(), ValidationError> {
    if model.services.is_empty() && model.tasks.is_empty() {
        return Err(ValidationError::UnsupportedValue {
            field: "services",
            expected: "at least one service or task",
            actual: "{}".to_string(),
        });
    }
    for (name, service) in &model.services {
        match service.containment {
            ContainmentRequirement::ProcessGroup | ContainmentRequirement::ProcessTree => {}
        }
        validate_service_endpoints(name, service)?;
        validate_service_lifecycle(service)?;
    }
    validate_connects_to(model)?;
    Ok(())
}

/// A service binds zero or more endpoints (durable is not listening); each map
/// key equals its endpointId, the primary is present iff any endpoint is
/// declared and names one, and no endpointId collides with a `connectsTo`
/// serviceId (the two share the `${port:<name>}` namespace, resolved
/// own-endpoint first, so a collision would be ambiguous).
fn validate_service_endpoints(name: &str, service: &ServiceSpec) -> Result<(), ValidationError> {
    if service.endpoints.is_empty() {
        if service.primary_endpoint.is_some() {
            return Err(ValidationError::UnsupportedValue {
                field: "services.primaryEndpoint",
                expected: "no primary endpoint on an endpoint-less service",
                actual: format!("{name}: {:?}", service.primary_endpoint),
            });
        }
        return Ok(());
    }
    for (id, endpoint) in &service.endpoints {
        require_non_empty("services.endpoints.endpointId", &endpoint.endpoint_id)?;
        if endpoint.endpoint_id != *id {
            return Err(ValidationError::UnsupportedValue {
                field: "services.endpoints",
                expected: "map key equal to endpointId",
                actual: format!("{id} -> {}", endpoint.endpoint_id),
            });
        }
        if service
            .connects_to
            .iter()
            .any(|target| target.as_str() == id)
        {
            return Err(ValidationError::UnsupportedValue {
                field: "services.endpoints",
                expected: "endpointId distinct from every connectsTo serviceId",
                actual: format!("{name}: {id}"),
            });
        }
    }
    match &service.primary_endpoint {
        Some(primary) if service.endpoints.contains_key(primary) => Ok(()),
        other => Err(ValidationError::UnsupportedValue {
            field: "services.primaryEndpoint",
            expected: "a declared endpoint id",
            actual: format!("{name}: {other:?}"),
        }),
    }
}

/// `connectsTo` targets must be declared services and the wiring graph must be
/// acyclic; the same checks the Nix compiler enforces, repeated fail-closed at
/// the admission boundary.
fn validate_connects_to(model: &Model) -> Result<(), ValidationError> {
    for (name, service) in &model.services {
        for target in service.connects_to.iter() {
            if !model.services.contains_key(target.as_str()) {
                return Err(ValidationError::UnsupportedValue {
                    field: "services.connectsTo",
                    expected: "a declared service",
                    actual: format!("{name} -> {target}"),
                });
            }
        }
    }
    for start in model.services.keys() {
        let mut seen = Vec::new();
        if connects_to_reaches(model, start, start, &mut seen) {
            return Err(ValidationError::UnsupportedValue {
                field: "services.connectsTo",
                expected: "an acyclic wiring graph",
                actual: format!("cycle through {start}"),
            });
        }
    }
    Ok(())
}

fn connects_to_reaches<'a>(
    model: &'a Model,
    start: &str,
    current: &str,
    seen: &mut Vec<&'a ServiceId>,
) -> bool {
    let Some(service) = model.services.get(current) else {
        return false;
    };
    service.connects_to.iter().any(|target| {
        target.as_str() == start
            || (!seen.contains(&target) && {
                seen.push(target);
                connects_to_reaches(model, start, target.as_str(), seen)
            })
    })
}

/// The lifecycle's per-class shape and the endpoint/probe wiring are now
/// guaranteed by the types (single endpoint, inline probe timings, loopback
/// host), so only non-empty value checks on ids/terminals remain.
fn validate_service_lifecycle(service: &ServiceSpec) -> Result<(), ValidationError> {
    for (operation_id, terminal) in lifecycle_ops(&service.lifecycle) {
        require_non_empty("lifecycle.operationId", operation_id.as_str())?;
        require_non_empty("lifecycle.terminal.success", &terminal.success)?;
        require_non_empty("lifecycle.terminal.failure", &terminal.failure)?;
    }
    Ok(())
}

/// The (operationId, terminal) pair of each lifecycle op, in canonical order.
fn lifecycle_ops(lifecycle: &Lifecycle) -> [(&OperationId, &TerminalSemantics); 6] {
    [
        (&lifecycle.prepare.operation_id, &lifecycle.prepare.terminal),
        (&lifecycle.start.operation_id, &lifecycle.start.terminal),
        (&lifecycle.ready.operation_id, &lifecycle.ready.terminal),
        (&lifecycle.health.operation_id, &lifecycle.health.terminal),
        (&lifecycle.stop.operation_id, &lifecycle.stop.terminal),
        (&lifecycle.clean.operation_id, &lifecycle.clean.terminal),
    ]
}

/// Kind/field coherence for the discriminated task record: a leaf carries an
/// operation id, an invocation, and an exit policy and no steps; a composite
/// carries non-empty steps and none of the leaf machinery. Step names are
/// evidence path segments, so they exclude `.`.
fn validate_tasks(model: &Model) -> Result<(), ValidationError> {
    for (id, task) in &model.tasks {
        match task.kind {
            TaskKind::Leaf => {
                let operation_id =
                    task.operation_id
                        .as_ref()
                        .ok_or(ValidationError::UnsupportedValue {
                            field: "tasks.operationId",
                            expected: "an operation id on a leaf task",
                            actual: format!("{id}: none"),
                        })?;
                require_non_empty("tasks.operationId", operation_id.as_str())?;
                if task.invocation.is_none() {
                    return Err(ValidationError::UnsupportedValue {
                        field: "tasks.invocation",
                        expected: "an invocation on a leaf task",
                        actual: format!("{id}: none"),
                    });
                }
                let exit_policy =
                    task.exit_policy
                        .as_ref()
                        .ok_or(ValidationError::UnsupportedValue {
                            field: "tasks.exitPolicy",
                            expected: "an exit policy on a leaf task",
                            actual: format!("{id}: none"),
                        })?;
                if exit_policy.success_codes.is_empty() {
                    return Err(ValidationError::UnsupportedValue {
                        field: "tasks.exitPolicy.successCodes",
                        expected: "at least one success code",
                        actual: "[]".to_string(),
                    });
                }
                if !task.steps.is_empty() {
                    return Err(ValidationError::UnsupportedValue {
                        field: "tasks.steps",
                        expected: "no steps on a leaf task",
                        actual: format!("{id}: {} steps", task.steps.len()),
                    });
                }
            }
            TaskKind::Composite => {
                if task.steps.is_empty() {
                    return Err(ValidationError::UnsupportedValue {
                        field: "tasks.steps",
                        expected: "at least one step on a composite task",
                        actual: format!("{id}: {{}}"),
                    });
                }
                if task.operation_id.is_some()
                    || task.invocation.is_some()
                    || task.exit_policy.is_some()
                    || !task.requires.is_empty()
                    || !task.artifact_refs.is_empty()
                    || !task.log_refs.is_empty()
                    || !task.summary_refs.is_empty()
                {
                    return Err(ValidationError::UnsupportedValue {
                        field: "tasks",
                        expected: "a composite carries only steps (leaves own invocation, requires, exitPolicy, refs)",
                        actual: id.clone(),
                    });
                }
                for step_name in task.steps.keys() {
                    require_step_safe_id("tasks.steps", step_name)?;
                }
            }
        }
    }
    Ok(())
}

/// Step names and task/service ids are step-path segments
/// (docs/DERIVATION_SPEC.md §1): path-safe AND dot-free, so paths parse
/// unambiguously on `.`.
fn require_step_safe_id(field: &'static str, id: &str) -> Result<(), ValidationError> {
    let mut chars = id.chars();
    let valid = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedValue {
            field,
            expected: "an id matching [A-Za-z0-9][A-Za-z0-9_-]*",
            actual: id.to_string(),
        })
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
