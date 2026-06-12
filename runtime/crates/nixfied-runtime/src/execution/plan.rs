//! The run planner: a pure function of `(ExecutionModel, selected task, slot)`
//! that derives the service union, assigns service ports, and orders the
//! flattened nodes — or rejects when no concrete executable plan exists.
//! Admission runs it over every slot and every declared task so that
//! "admitted" means "a concrete plan exists"; `run` recomputes the same
//! function.

use std::collections::{BTreeMap, BTreeSet};

use nixfied_model::{NodeId, ServiceId, TaskId};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::types::{ExecutionModel, PortWindow};

/// A concrete, executable plan for one slot: services bound to ports in start
/// order, and tasks in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub services: Vec<ServiceBinding>,
    pub nodes: Vec<PlanNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceBinding {
    pub service_name: ServiceId,
    /// The port assigned to each of the service's endpoints, keyed by endpointId.
    /// Ports come from the service's contiguous block in the slot window, so every
    /// modelled listener has a reserved, conflict-checked port.
    pub endpoint_ports: BTreeMap<String, u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanNode {
    pub node_id: NodeId,
    pub task_id: TaskId,
}

pub fn plan(model: &ExecutionModel, task: &TaskId, slot: u32) -> RuntimeResult<RunPlan> {
    let window = model.slot_windows.get(&slot).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("slot {slot} has no candidate port window"),
        )
    })?;

    let nodes = flatten_task(model, task)?;
    // The derived service union (docs/DERIVATION_SPEC.md §3): the flattened
    // leaves' requires, closed over connectsTo, in canonical (byte) order —
    // ports are assigned from the sorted set, so a service's port depends only
    // on membership, never on authoring order.
    let mut union: BTreeSet<ServiceId> = nodes
        .iter()
        .filter_map(|node| model.tasks.get(&node.task_id))
        .flat_map(|leaf| leaf.requires.iter().cloned())
        .collect();
    loop {
        let additions: Vec<ServiceId> = union
            .iter()
            .filter_map(|name| model.services.get(name))
            .flat_map(|service| service.connects_to.iter().cloned())
            .filter(|target| !union.contains(target))
            .collect();
        if additions.is_empty() {
            break;
        }
        union.extend(additions);
    }
    let service_names: Vec<ServiceId> = union.into_iter().collect();

    // Ports are assigned in the union's canonical order (stable, predictable
    // addressing — authoring order never moves a service's port), then the
    // bindings are reordered for start so every connectsTo dependency is ready
    // before its dependent spawns. Validation proved the wiring graph acyclic,
    // and the union is connectsTo-closed by construction, so the sort always
    // completes.
    let services = order_for_start(assign_ports(&service_names, model, *window, slot)?, model);
    Ok(RunPlan { services, nodes })
}

/// Stable topological order over the connectsTo graph: among services whose
/// dependencies are all started, declared order wins, so a wiring-free model
/// keeps exactly its declared start order.
fn order_for_start(bindings: Vec<ServiceBinding>, model: &ExecutionModel) -> Vec<ServiceBinding> {
    let mut remaining = bindings;
    let mut ordered = Vec::with_capacity(remaining.len());
    let mut started: BTreeSet<ServiceId> = BTreeSet::new();
    while !remaining.is_empty() {
        let ready = remaining.iter().position(|binding| {
            model
                .services
                .get(&binding.service_name)
                .map(|service| {
                    service.connects_to.iter().all(|target| {
                        started.contains(target)
                            || !remaining.iter().any(|other| other.service_name == *target)
                    })
                })
                .unwrap_or(true)
        });
        // Lowering guarantees acyclicity; a missing ready node would mean a
        // cycle leaked through, so falling back to declared order is the only
        // defensive option left.
        let next = remaining.remove(ready.unwrap_or(0));
        started.insert(next.service_name.clone());
        ordered.push(next);
    }
    ordered
}

/// Assign each service a contiguous block of ports from the window — one per
/// endpoint, in endpointId order — advancing a single cursor across services in
/// declared order. Proves the window has capacity for every modelled listener, so
/// a service can never run an unreserved port.
fn assign_ports(
    service_names: &[ServiceId],
    model: &ExecutionModel,
    window: PortWindow,
    slot: u32,
) -> RuntimeResult<Vec<ServiceBinding>> {
    let mut bindings = Vec::with_capacity(service_names.len());
    let mut cursor = window.start;
    let mut exhausted = false;
    for service_name in service_names {
        // Lowering proved every selected service resolves; an absent one would be
        // a planner/lowering skew, so treat it as an empty endpoint set.
        let endpoint_ids = model
            .services
            .get(service_name)
            .map(|service| service.endpoints.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut endpoint_ports = BTreeMap::new();
        for endpoint_id in endpoint_ids {
            if exhausted || cursor > window.end {
                exhausted = true;
                break;
            }
            endpoint_ports.insert(endpoint_id, cursor);
            match cursor.checked_add(1) {
                Some(next) => cursor = next,
                None => exhausted = true,
            }
        }
        bindings.push(ServiceBinding {
            service_name: service_name.clone(),
            endpoint_ports,
        });
    }
    let demand: usize = bindings
        .iter()
        .map(|binding| binding.endpoint_ports.len())
        .sum();
    let assigned: usize = service_names
        .iter()
        .filter_map(|name| model.services.get(name))
        .map(|service| service.endpoints.len())
        .sum();
    if exhausted || demand != assigned {
        return Err(RuntimeError::new(
            ErrorCode::PortConflict,
            format!(
                "slot {slot} candidate window {}-{} cannot host {assigned} endpoints across {} services",
                window.start,
                window.end,
                service_names.len()
            ),
        ));
    }
    Ok(bindings)
}

/// Flatten one selected task into ordered plan nodes with stable step paths
/// (docs/DERIVATION_SPEC.md §2): a leaf is a single node whose path is the task
/// id; a composite emits its steps depth-first in canonical step-name order,
/// each node's path `<root>.<step>...<step>`. A step's `dependsOn` constrains
/// every node of its subtree on **every** node of the dependency's subtree
/// (composite success is conjunction). The returned order is the deterministic
/// topological order: emission order, earliest node whose dependencies are
/// already placed first. Cycles through nesting are rejected here — admission
/// proves every selection plans, so a cyclic reference never survives it.
fn flatten_task(model: &ExecutionModel, root: &TaskId) -> RuntimeResult<Vec<PlanNode>> {
    struct FlatNode {
        step_path: String,
        leaf: TaskId,
        depends_on: BTreeSet<String>,
    }

    fn emit(
        model: &ExecutionModel,
        task: &TaskId,
        path: String,
        visiting: &mut Vec<TaskId>,
        out: &mut Vec<FlatNode>,
    ) -> RuntimeResult<()> {
        if model.tasks.contains_key(task) {
            out.push(FlatNode {
                step_path: path,
                leaf: task.clone(),
                depends_on: BTreeSet::new(),
            });
            return Ok(());
        }
        let Some(composite) = model.composites.get(task) else {
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("task {task} is missing"),
            ));
        };
        if visiting.contains(task) {
            let chain = visiting
                .iter()
                .map(|id| id.as_str())
                .chain(std::iter::once(task.as_str()))
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("task reference cycle: {chain}"),
            ));
        }
        visiting.push(task.clone());
        // Emit each step's subtree (steps are already in canonical order),
        // recording its node range so sibling dependencies can be applied to
        // whole subtrees afterwards (dependsOn may name a later sibling).
        let mut ranges: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for step in &composite.steps {
            let start = out.len();
            emit(
                model,
                &step.task,
                format!("{path}.{}", step.name),
                visiting,
                out,
            )?;
            ranges.insert(step.name.as_str(), (start, out.len()));
        }
        for step in &composite.steps {
            let mut dependency_paths = BTreeSet::new();
            for dependency in &step.depends_on {
                // Lowering proved dependsOn names a sibling step.
                let (start, end) = ranges[dependency.as_str()];
                dependency_paths.extend(out[start..end].iter().map(|node| node.step_path.clone()));
            }
            if dependency_paths.is_empty() {
                continue;
            }
            let (start, end) = ranges[step.name.as_str()];
            for node in &mut out[start..end] {
                node.depends_on.extend(dependency_paths.iter().cloned());
            }
        }
        visiting.pop();
        Ok(())
    }

    let mut flat = Vec::new();
    emit(
        model,
        root,
        root.as_str().to_string(),
        &mut Vec::new(),
        &mut flat,
    )?;

    // Deterministic topological order over the flattened nodes.
    let mut ordered = Vec::with_capacity(flat.len());
    let mut placed: BTreeSet<String> = BTreeSet::new();
    let mut remaining: Vec<FlatNode> = flat;
    while !remaining.is_empty() {
        let Some(index) = remaining
            .iter()
            .position(|node| node.depends_on.iter().all(|dep| placed.contains(dep)))
        else {
            // A sibling dependsOn cycle: every unplaced node waits on another.
            let stuck = remaining
                .iter()
                .map(|node| node.step_path.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("task {root} has a step dependency cycle through: {stuck}"),
            ));
        };
        let node = remaining.remove(index);
        placed.insert(node.step_path.clone());
        ordered.push(PlanNode {
            node_id: NodeId::new(&node.step_path),
            task_id: node.leaf,
        });
    }
    Ok(ordered)
}

/// Prove a concrete plan exists for every slot in the policy range and every
/// declared task. Admission calls this so "admitted" implies "runnable for any
/// slot/task the user can select" — including composite acyclicity and the
/// derived union's port feasibility.
pub fn prove_all_plans_feasible(model: &ExecutionModel) -> RuntimeResult<()> {
    for &slot in model.slot_windows.keys() {
        for task in model.tasks.keys().chain(model.composites.keys()) {
            plan(model, task, slot)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::types::*;
    use std::collections::BTreeMap;
    use std::time::Duration;

    use nixfied_model::{ContainmentRequirement, OperationId, ServiceId, TaskId};

    use crate::execution::ServiceIdentity;

    fn op_meta(id: &str) -> OpMeta {
        OpMeta {
            operation_id: OperationId::new(id),
            terminal_success: "ok".to_string(),
            terminal_failure: "fail".to_string(),
        }
    }

    fn resolved_exec() -> ResolvedInvocation {
        ResolvedInvocation {
            executable: "/bin/svc".to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: ".".to_string(),
            stdin: StdinPolicy::Null,
            timeout: Duration::from_millis(1000),
            tool_roots: Vec::new(),
        }
    }

    fn tcp_probe() -> TcpProbe {
        TcpProbe {
            label: "ready".to_string(),
            timeout: Duration::from_millis(1000),
            retry_interval: Duration::from_millis(100),
            max_attempts: 10,
        }
    }

    fn service(name: &str) -> ExecService {
        ExecService {
            name: ServiceId::new(name),
            prepare: PrepareOp {
                meta: op_meta("prepare"),
                exec: None,
            },
            start: StartOp {
                meta: op_meta("start"),
                exec: resolved_exec(),
            },
            ready: ReadyOp {
                meta: op_meta("ready"),
                probe: Probe::Tcp(tcp_probe()),
            },
            health: HealthOp {
                meta: op_meta("health"),
                probe: Probe::Tcp(tcp_probe()),
            },
            stop: StopOp {
                meta: op_meta("stop"),
                signal: StopSignal::Term,
                timeout: Duration::from_millis(5000),
            },
            clean: CleanOp {
                meta: op_meta("clean"),
            },
            endpoints: BTreeMap::from([(
                "e".to_string(),
                ResolvedEndpoint {
                    endpoint_id: "e".to_string(),
                    host: LoopbackHost::parse("127.0.0.1").unwrap(),
                },
            )]),
            primary_endpoint: Some("e".to_string()),
            connects_to: Vec::new(),
            containment: ContainmentRequirement::ProcessGroup,
            identity: ServiceIdentity {
                endpoint_identity_hash: "e".to_string(),
                state_identity_hash: "s".to_string(),
                runtime_compatibility_hash: "r".to_string(),
                target_identity_hash: "t".to_string(),
            },
        }
    }

    /// A model whose `all` leaf requires the given services — selecting it
    /// derives exactly that union.
    fn model(
        services: Vec<&str>,
        required: Vec<&str>,
        windows: Vec<(u32, u16, u16)>,
    ) -> ExecutionModel {
        let mut leaf = leaf_task("all");
        leaf.requires = required.into_iter().map(ServiceId::new).collect();
        ExecutionModel {
            services: services
                .into_iter()
                .map(|name| (ServiceId::new(name), service(name)))
                .collect(),
            tasks: BTreeMap::from([(TaskId::new("all"), leaf)]),
            composites: BTreeMap::new(),
            slot_windows: windows
                .into_iter()
                .map(|(slot, start, end)| (slot, PortWindow { start, end }))
                .collect(),
        }
    }

    #[test]
    fn assigns_ports_from_window_start_in_order() {
        let em = model(vec!["a", "b"], vec!["a", "b"], vec![(0, 23080, 23090)]);
        let plan = plan(&em, &TaskId::new("all"), 0).expect("plan exists");
        assert_eq!(
            plan.services,
            vec![
                ServiceBinding {
                    service_name: ServiceId::new("a"),
                    endpoint_ports: BTreeMap::from([("e".to_string(), 23080)]),
                },
                ServiceBinding {
                    service_name: ServiceId::new("b"),
                    endpoint_ports: BTreeMap::from([("e".to_string(), 23081)]),
                },
            ]
        );
    }

    #[test]
    fn rejects_when_window_cannot_host_all_services() {
        let em = model(vec!["a", "b"], vec!["a", "b"], vec![(0, 23080, 23080)]);
        let error = plan(&em, &TaskId::new("all"), 0).expect_err("window too small");
        assert_eq!(error.code, ErrorCode::PortConflict);
    }

    #[test]
    fn feasibility_is_checked_per_slot() {
        // Slot 0 fits two services; slot 1's window holds only one.
        let em = model(
            vec!["a", "b"],
            vec!["a", "b"],
            vec![(0, 23080, 23090), (1, 24000, 24000)],
        );
        assert!(plan(&em, &TaskId::new("all"), 0).is_ok());
        assert_eq!(
            plan(&em, &TaskId::new("all"), 1)
                .expect_err("slot 1 too small")
                .code,
            ErrorCode::PortConflict
        );
        assert_eq!(
            prove_all_plans_feasible(&em)
                .expect_err("not all slots feasible")
                .code,
            ErrorCode::PortConflict
        );
    }

    fn leaf_task(name: &str) -> ExecTask {
        ExecTask {
            task_id: TaskId::new(name),
            exec: resolved_exec(),
            requires: Vec::new(),
            success_codes: vec![0],
        }
    }

    fn composite(name: &str, steps: Vec<(&str, &str, Vec<&str>)>) -> ExecComposite {
        ExecComposite {
            task_id: TaskId::new(name),
            steps: steps
                .into_iter()
                .map(|(step, task, deps)| ExecStep {
                    name: step.to_string(),
                    task: TaskId::new(task),
                    depends_on: deps.into_iter().map(String::from).collect(),
                })
                .collect(),
        }
    }

    fn vector_model(leaves: Vec<&str>, composites: Vec<ExecComposite>) -> ExecutionModel {
        let mut em = model(vec![], vec![], vec![(0, 23080, 23090)]);
        em.tasks.clear();
        for leaf in leaves {
            em.tasks.insert(TaskId::new(leaf), leaf_task(leaf));
        }
        for comp in composites {
            em.composites.insert(comp.task_id.clone(), comp);
        }
        em
    }

    fn plan_paths(em: &ExecutionModel, root: &str) -> Vec<String> {
        plan(em, &TaskId::new(root), 0)
            .expect("plan exists")
            .nodes
            .iter()
            .map(|node| node.node_id.as_str().to_string())
            .collect()
    }

    /// Golden vector V1 (docs/DERIVATION_SPEC.md §6): nesting, step paths,
    /// canonical step order, whole-subtree dependencies.
    #[test]
    fn flattening_vector_v1_nesting_and_step_paths() {
        let em = vector_model(
            vec!["fmt", "clippy", "tests"],
            vec![
                composite(
                    "check",
                    vec![("fmt", "fmt", vec![]), ("clippy", "clippy", vec!["fmt"])],
                ),
                composite(
                    "ci",
                    vec![
                        ("check", "check", vec![]),
                        ("tests", "tests", vec!["check"]),
                    ],
                ),
            ],
        );
        // Emission order is check < tests, clippy emitted before fmt (byte
        // order), but fmt orders first (clippy depends on it); ci.tests
        // depends on every node under ci.check.
        assert_eq!(
            plan_paths(&em, "ci"),
            vec!["ci.check.fmt", "ci.check.clippy", "ci.tests"]
        );
    }

    /// Golden vector V2: the same task referenced twice flattens twice with
    /// distinct step paths.
    #[test]
    fn flattening_vector_v2_run_once_is_per_step() {
        let em = vector_model(
            vec!["unit"],
            vec![composite(
                "twice",
                vec![("again", "unit", vec!["first"]), ("first", "unit", vec![])],
            )],
        );
        assert_eq!(plan_paths(&em, "twice"), vec!["twice.first", "twice.again"]);
    }

    /// Golden vector V3: selecting a leaf directly yields one node whose step
    /// path is the task id.
    #[test]
    fn flattening_vector_v3_leaf_selection() {
        let em = vector_model(vec!["fmt"], vec![]);
        assert_eq!(plan_paths(&em, "fmt"), vec!["fmt"]);
    }

    #[test]
    fn flattening_rejects_a_task_reference_cycle() {
        let em = vector_model(
            vec![],
            vec![
                composite("a", vec![("to-b", "b", vec![])]),
                composite("b", vec![("to-a", "a", vec![])]),
            ],
        );
        let error = plan(&em, &TaskId::new("a"), 0).expect_err("cycle must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("cycle"), "{}", error.message);
    }

    #[test]
    fn flattening_rejects_a_sibling_depends_on_cycle() {
        let em = vector_model(
            vec!["unit"],
            vec![composite(
                "spin",
                vec![("x", "unit", vec!["y"]), ("y", "unit", vec!["x"])],
            )],
        );
        let error = plan(&em, &TaskId::new("spin"), 0).expect_err("cycle must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("cycle"), "{}", error.message);
    }

    #[test]
    fn start_order_honors_connects_to_but_ports_stay_declared() {
        // app sorts first (and keeps the first port), but connects to db, so
        // db must start first.
        let mut em = model(
            vec!["app", "db"],
            vec!["app", "db"],
            vec![(0, 23080, 23090)],
        );
        em.services
            .get_mut(&ServiceId::new("app"))
            .expect("app exists")
            .connects_to = vec![ServiceId::new("db")];
        let plan = plan(&em, &TaskId::new("all"), 0).expect("plan exists");
        assert_eq!(
            plan.services,
            vec![
                ServiceBinding {
                    service_name: ServiceId::new("db"),
                    endpoint_ports: BTreeMap::from([("e".to_string(), 23081)]),
                },
                ServiceBinding {
                    service_name: ServiceId::new("app"),
                    endpoint_ports: BTreeMap::from([("e".to_string(), 23080)]),
                },
            ]
        );
    }

    #[test]
    fn assigns_a_contiguous_block_per_multi_endpoint_service() {
        // `multi` declares three endpoints, so it consumes a three-port block; the
        // single-endpoint `solo` takes the next port after the block.
        let mut em = model(
            vec!["multi", "solo"],
            vec!["multi", "solo"],
            vec![(0, 23080, 23090)],
        );
        let multi = em
            .services
            .get_mut(&ServiceId::new("multi"))
            .expect("multi exists");
        for id in ["a", "b", "c"] {
            multi.endpoints.insert(
                id.to_string(),
                ResolvedEndpoint {
                    endpoint_id: id.to_string(),
                    host: LoopbackHost::parse("127.0.0.1").unwrap(),
                },
            );
        }
        multi.endpoints.remove("e");
        multi.primary_endpoint = Some("a".to_string());
        let plan = plan(&em, &TaskId::new("all"), 0).expect("plan exists");
        let multi_ports = &plan
            .services
            .iter()
            .find(|b| b.service_name.as_str() == "multi")
            .expect("multi binding")
            .endpoint_ports;
        assert_eq!(
            multi_ports,
            &BTreeMap::from([
                ("a".to_string(), 23080),
                ("b".to_string(), 23081),
                ("c".to_string(), 23082),
            ])
        );
        let solo_ports = &plan
            .services
            .iter()
            .find(|b| b.service_name.as_str() == "solo")
            .expect("solo binding")
            .endpoint_ports;
        assert_eq!(solo_ports, &BTreeMap::from([("e".to_string(), 23083)]));
    }

    #[test]
    fn rejects_when_window_cannot_host_all_endpoints() {
        // Two single-endpoint services need two ports; a one-port window cannot.
        let em = model(vec!["a", "b"], vec!["a", "b"], vec![(0, 23080, 23080)]);
        assert_eq!(
            plan(&em, &TaskId::new("all"), 0)
                .expect_err("window too small for the endpoint block")
                .code,
            ErrorCode::PortConflict
        );
    }
}
