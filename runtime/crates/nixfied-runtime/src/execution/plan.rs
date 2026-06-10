//! The run planner: a pure function of `(ExecutionModel, selection, slot)` that
//! assigns service ports and orders tasks, or rejects when no concrete executable
//! plan exists. Admission runs it over every slot/selection so that "admitted"
//! means "a concrete plan exists"; `run` recomputes the same function.

use std::collections::{BTreeMap, BTreeSet};

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
    pub service_name: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanNode {
    pub node_id: String,
    pub task_id: String,
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
                    node_id: task_id.clone(),
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

    let services = assign_ports(&service_names, *window, slot)?;
    Ok(RunPlan {
        services,
        nodes,
        workflow_id,
    })
}

/// Assign each service the next port in the window (start + index), proving the
/// window has capacity for every service.
fn assign_ports(
    service_names: &[String],
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
                node.depends_on.iter().map(String::as_str).collect(),
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
                node_id: node_id.to_string(),
                task_id: task_by_node[node_id].to_string(),
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

    use nixfied_model::{ContainmentRequirement, ServiceIdentity};

    fn op_meta(id: &str) -> OpMeta {
        OpMeta {
            operation_id: id.to_string(),
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
            name: name.to_string(),
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
                .map(|name| (name.to_string(), service(name)))
                .collect(),
            tasks: BTreeMap::new(),
            environment: ExecEnvironment {
                services: env_services.into_iter().map(String::from).collect(),
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
                    service_name: "a".to_string(),
                    port: 38080
                },
                ServiceBinding {
                    service_name: "b".to_string(),
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
                services_required: vec!["a".to_string()],
                nodes: vec![
                    ExecWorkflowNode {
                        node_id: "second".to_string(),
                        task_id: "t".to_string(),
                        depends_on: vec!["first".to_string()],
                    },
                    ExecWorkflowNode {
                        node_id: "first".to_string(),
                        task_id: "t".to_string(),
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
                services_required: vec!["a".to_string()],
                nodes: vec![
                    ExecWorkflowNode {
                        node_id: "x".to_string(),
                        task_id: "t".to_string(),
                        depends_on: vec!["y".to_string()],
                    },
                    ExecWorkflowNode {
                        node_id: "y".to_string(),
                        task_id: "t".to_string(),
                        depends_on: vec!["x".to_string()],
                    },
                ],
            },
        );
        assert!(plan(&em, Selection::Workflow("wf"), 0).is_err());
    }
}
