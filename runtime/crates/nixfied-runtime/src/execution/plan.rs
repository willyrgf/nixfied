//! The run planner: a pure function of `(ExecutionModel, selection, slot)` that
//! assigns service ports and orders tasks, or rejects when no concrete executable
//! plan exists. Admission runs it over every slot/selection so that "admitted"
//! means "a concrete plan exists"; `run` recomputes the same function.

use std::collections::{BTreeMap, BTreeSet};

use nixfied_model::{NodeId, ServiceId, TaskId};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::types::{ExecWorkflow, ExecutionModel, PortWindow};

/// Which program a run drives: the environment's services + tasks, or a workflow.
#[derive(Debug, Clone, Copy)]
pub enum Selection<'a> {
    Environment,
    Workflow(&'a str),
}

/// A concrete, executable plan for one slot: services bound to ports in start
/// order, and tasks in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub services: Vec<ServiceBinding>,
    pub nodes: Vec<PlanNode>,
    pub workflow_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceBinding {
    pub service_name: ServiceId,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanNode {
    pub node_id: NodeId,
    pub task_id: TaskId,
}

pub fn plan(model: &ExecutionModel, selection: Selection<'_>, slot: u32) -> RuntimeResult<RunPlan> {
    let window = model.slot_windows.get(&slot).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("slot {slot} has no candidate port window"),
        )
    })?;

    let (service_names, nodes, workflow_id) = match selection {
        Selection::Environment => (
            model.environment.services.clone(),
            model
                .environment
                .tasks
                .iter()
                .map(|task_id| PlanNode {
                    node_id: NodeId::new(task_id.as_str()),
                    task_id: task_id.clone(),
                })
                .collect(),
            None,
        ),
        Selection::Workflow(id) => {
            let workflow = model.workflows.get(id).ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::ModelAdmission,
                    format!("workflow {id} is missing"),
                )
            })?;
            let nodes = topological_order(workflow).ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::ModelAdmission,
                    format!("workflow {id} graph is not acyclic"),
                )
            })?;
            (
                workflow.services_required.clone(),
                nodes,
                Some(id.to_string()),
            )
        }
    };

    // Ports are assigned by *declared* order (stable, predictable addressing —
    // wiring changes never move a service's port), then the bindings are
    // reordered for start so every connectsTo dependency is ready before its
    // dependent spawns. Lowering proved the graph acyclic and closed under the
    // selection, so the sort always completes.
    let services = order_for_start(assign_ports(&service_names, *window, slot)?, model);
    Ok(RunPlan {
        services,
        nodes,
        workflow_id,
    })
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

/// Assign each service the next port in the window (start + index), proving the
/// window has capacity for every service.
fn assign_ports(
    service_names: &[ServiceId],
    window: PortWindow,
    slot: u32,
) -> RuntimeResult<Vec<ServiceBinding>> {
    let mut bindings = Vec::with_capacity(service_names.len());
    for (index, service_name) in service_names.iter().enumerate() {
        let offset = u16::try_from(index).ok().filter(|offset| {
            window
                .start
                .checked_add(*offset)
                .is_some_and(|port| port <= window.end)
        });
        let Some(offset) = offset else {
            return Err(RuntimeError::new(
                ErrorCode::PortConflict,
                format!(
                    "slot {slot} candidate window {}-{} cannot host {} services",
                    window.start,
                    window.end,
                    service_names.len()
                ),
            ));
        };
        let port = window.start + offset;
        bindings.push(ServiceBinding {
            service_name: service_name.clone(),
            port,
        });
    }
    Ok(bindings)
}

/// Deterministic topological order of workflow nodes; `None` on a cycle.
fn topological_order(workflow: &ExecWorkflow) -> Option<Vec<PlanNode>> {
    let mut pending: BTreeMap<&str, BTreeSet<&str>> = workflow
        .nodes
        .iter()
        .map(|node| {
            (
                node.node_id.as_str(),
                node.depends_on.iter().map(|id| id.as_str()).collect(),
            )
        })
        .collect();
    let task_by_node: BTreeMap<&str, &str> = workflow
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node.task_id.as_str()))
        .collect();
    let mut ordered = Vec::new();
    while !pending.is_empty() {
        let ready: Vec<&str> = pending
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(id, _)| *id)
            .collect();
        if ready.is_empty() {
            return None;
        }
        for node_id in ready {
            pending.remove(node_id);
            for deps in pending.values_mut() {
                deps.remove(node_id);
            }
            ordered.push(PlanNode {
                node_id: NodeId::new(node_id),
                task_id: TaskId::new(task_by_node[node_id]),
            });
        }
    }
    Some(ordered)
}

/// Prove a concrete plan exists for every slot in the policy range and for the
/// environment and every workflow selection. Admission calls this so "admitted"
/// implies "runnable for any slot/selection the user can pick".
pub fn prove_all_plans_feasible(model: &ExecutionModel) -> RuntimeResult<()> {
    for &slot in model.slot_windows.keys() {
        plan(model, Selection::Environment, slot)?;
        for workflow_id in model.workflows.keys() {
            plan(model, Selection::Workflow(workflow_id), slot)?;
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

    use nixfied_model::{
        ContainmentRequirement, NodeId, OperationId, ServiceId, ServiceIdentity, TaskId,
    };

    fn op_meta(id: &str) -> OpMeta {
        OpMeta {
            operation_id: OperationId::new(id),
            terminal_success: "ok".to_string(),
            terminal_failure: "fail".to_string(),
        }
    }

    fn resolved_exec() -> ResolvedExec {
        ResolvedExec {
            executable: "/bin/svc".to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: ".".to_string(),
            stdin: StdinPolicy::Null,
            timeout: Duration::from_millis(1000),
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
                probe: tcp_probe(),
            },
            health: HealthOp {
                meta: op_meta("health"),
                probe: tcp_probe(),
            },
            stop: StopOp {
                meta: op_meta("stop"),
                signal: StopSignal::Term,
                timeout: Duration::from_millis(5000),
            },
            clean: CleanOp {
                meta: op_meta("clean"),
            },
            endpoint: ResolvedEndpoint {
                endpoint_id: "e".to_string(),
                host: LoopbackHost::parse("127.0.0.1").unwrap(),
            },
            connects_to: Vec::new(),
            containment: ContainmentRequirement::ProcessGroup,
            identity: ServiceIdentity {
                service_address_hash: "a".to_string(),
                endpoint_identity_hash: "e".to_string(),
                state_identity_hash: "s".to_string(),
                runtime_compatibility_hash: "r".to_string(),
                target_identity_hash: "t".to_string(),
            },
        }
    }

    fn model(
        services: Vec<&str>,
        env_services: Vec<&str>,
        windows: Vec<(u32, u16, u16)>,
    ) -> ExecutionModel {
        ExecutionModel {
            services: services
                .into_iter()
                .map(|name| (ServiceId::new(name), service(name)))
                .collect(),
            tasks: BTreeMap::new(),
            environment: ExecEnvironment {
                services: env_services.into_iter().map(ServiceId::new).collect(),
                tasks: Vec::new(),
            },
            workflows: BTreeMap::new(),
            slot_windows: windows
                .into_iter()
                .map(|(slot, start, end)| (slot, PortWindow { start, end }))
                .collect(),
        }
    }

    #[test]
    fn assigns_ports_from_window_start_in_order() {
        let em = model(vec!["a", "b"], vec!["a", "b"], vec![(0, 38080, 38090)]);
        let plan = plan(&em, Selection::Environment, 0).expect("plan exists");
        assert_eq!(
            plan.services,
            vec![
                ServiceBinding {
                    service_name: ServiceId::new("a"),
                    port: 38080
                },
                ServiceBinding {
                    service_name: ServiceId::new("b"),
                    port: 38081
                },
            ]
        );
    }

    #[test]
    fn rejects_when_window_cannot_host_all_services() {
        let em = model(vec!["a", "b"], vec!["a", "b"], vec![(0, 38080, 38080)]);
        let error = plan(&em, Selection::Environment, 0).expect_err("window too small");
        assert_eq!(error.code, ErrorCode::PortConflict);
    }

    #[test]
    fn feasibility_is_checked_per_slot() {
        // Slot 0 fits two services; slot 1's window holds only one.
        let em = model(
            vec!["a", "b"],
            vec!["a", "b"],
            vec![(0, 38080, 38090), (1, 39000, 39000)],
        );
        assert!(plan(&em, Selection::Environment, 0).is_ok());
        assert_eq!(
            plan(&em, Selection::Environment, 1)
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

    #[test]
    fn workflow_nodes_are_topologically_ordered() {
        let mut em = model(vec!["a"], vec!["a"], vec![(0, 38080, 38090)]);
        em.workflows.insert(
            "wf".to_string(),
            ExecWorkflow {
                services_required: vec![ServiceId::new("a")],
                nodes: vec![
                    ExecWorkflowNode {
                        node_id: NodeId::new("second"),
                        task_id: TaskId::new("t"),
                        depends_on: vec![NodeId::new("first")],
                    },
                    ExecWorkflowNode {
                        node_id: NodeId::new("first"),
                        task_id: TaskId::new("t"),
                        depends_on: vec![],
                    },
                ],
            },
        );
        let plan = plan(&em, Selection::Workflow("wf"), 0).expect("workflow plans");
        let order: Vec<&str> = plan.nodes.iter().map(|n| n.node_id.as_str()).collect();
        assert_eq!(order, vec!["first", "second"]);
        assert_eq!(plan.workflow_id.as_deref(), Some("wf"));
    }

    #[test]
    fn rejects_a_cyclic_workflow() {
        let mut em = model(vec!["a"], vec!["a"], vec![(0, 38080, 38090)]);
        em.workflows.insert(
            "wf".to_string(),
            ExecWorkflow {
                services_required: vec![ServiceId::new("a")],
                nodes: vec![
                    ExecWorkflowNode {
                        node_id: NodeId::new("x"),
                        task_id: TaskId::new("t"),
                        depends_on: vec![NodeId::new("y")],
                    },
                    ExecWorkflowNode {
                        node_id: NodeId::new("y"),
                        task_id: TaskId::new("t"),
                        depends_on: vec![NodeId::new("x")],
                    },
                ],
            },
        );
        assert!(plan(&em, Selection::Workflow("wf"), 0).is_err());
    }

    #[test]
    fn start_order_honors_connects_to_but_ports_stay_declared() {
        // app is declared first (and keeps the first port), but connects to db,
        // so db must start first.
        let mut em = model(
            vec!["app", "db"],
            vec!["app", "db"],
            vec![(0, 23080, 23090)],
        );
        em.services
            .get_mut(&ServiceId::new("app"))
            .expect("app exists")
            .connects_to = vec![ServiceId::new("db")];
        let plan = plan(&em, Selection::Environment, 0).expect("plan exists");
        assert_eq!(
            plan.services,
            vec![
                ServiceBinding {
                    service_name: ServiceId::new("db"),
                    port: 23081
                },
                ServiceBinding {
                    service_name: ServiceId::new("app"),
                    port: 23080
                },
            ]
        );
    }
}
