//! One graph interpretation for candidate admission and selected execution.
use super::lower::Rejection;
use super::types::{ExecutableTask, ExecutionManifest, PortWindow, Program};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use nixfied_manifest::{NodeId, ServiceId, ServiceLifetime, TaskId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct RunPlan<'a> {
    pub service_lifetime: ServiceLifetime,
    pub services: Vec<ServiceBinding<'a>>,
    pub nodes: Vec<PlanNode<'a>>,
}
#[derive(Debug)]
pub struct ServiceBinding<'a> {
    pub service: &'a super::types::ExecService,
    pub prepare_nodes: Vec<PlanNode<'a>>,
    pub endpoint_ports: BTreeMap<String, u16>,
}
#[derive(Debug)]
pub struct PlanNode<'a> {
    pub node_id: NodeId,
    pub task: &'a super::types::ExecTask,
}

struct LogicalPlan<'a> {
    service_lifetime: ServiceLifetime,
    services: Vec<(&'a super::types::ExecService, Vec<PlanNode<'a>>)>,
    nodes: Vec<PlanNode<'a>>,
    slot_windows: &'a BTreeMap<u32, PortWindow>,
}

type Edges = BTreeMap<ServiceId, Vec<(ServiceId, &'static str)>>;
/// Temporary admission facts. Flattened nodes are discarded per task, never retained here.
pub(super) struct GraphFacts {
    required: BTreeMap<TaskId, BTreeSet<ServiceId>>,
}

fn direct_requirements(nodes: &[PlanNode<'_>]) -> BTreeSet<ServiceId> {
    nodes
        .iter()
        .flat_map(|node| node.task.requires.iter().cloned())
        .collect()
}

fn service_edges(program: &Program, bases: &BTreeMap<TaskId, BTreeSet<ServiceId>>) -> Edges {
    program
        .services
        .iter()
        .map(|(id, service)| {
            let mut edges: Vec<_> = service
                .connects_to
                .iter()
                .cloned()
                .map(|id| (id, "connectsTo"))
                .collect();
            if let Some(prepare) = &service.prepare {
                edges.extend(
                    bases[prepare]
                        .iter()
                        .cloned()
                        .map(|id| (id, "prepare requires")),
                );
            }
            (id.clone(), edges)
        })
        .collect()
}

fn close_union(mut union: BTreeSet<ServiceId>, edges: &Edges) -> BTreeSet<ServiceId> {
    let mut pending: Vec<_> = union.iter().cloned().collect();
    while let Some(id) = pending.pop() {
        for (target, _) in &edges[&id] {
            if union.insert(target.clone()) {
                pending.push(target.clone());
            }
        }
    }
    union
}

fn prove_service_cycles(edges: &Edges) -> RuntimeResult<()> {
    fn visit<'a>(
        edges: &'a Edges,
        start: &ServiceId,
        current: &ServiceId,
        trail: &mut Vec<(&'a ServiceId, &'static str)>,
        seen: &mut BTreeSet<&'a ServiceId>,
    ) -> Option<Vec<(&'a ServiceId, &'static str)>> {
        for (target, kind) in &edges[current] {
            if target == start {
                let mut cycle = trail.clone();
                cycle.push((target, kind));
                return Some(cycle);
            }
            if seen.insert(target) {
                trail.push((target, kind));
                if let Some(cycle) = visit(edges, start, target, trail, seen) {
                    return Some(cycle);
                }
                trail.pop();
            }
        }
        None
    }
    for start in edges.keys() {
        if let Some(cycle) = visit(edges, start, start, &mut Vec::new(), &mut BTreeSet::new()) {
            let mut rendered = start.to_string();
            for (target, kind) in cycle {
                rendered.push_str(&format!(" -[{kind}]-> {target}"));
            }
            return Err(Rejection::ServiceGraphCycle { cycle: rendered }.into());
        }
    }
    Ok(())
}

pub(super) fn prove_graph(program: &Program) -> RuntimeResult<GraphFacts> {
    let mut bases = BTreeMap::new();
    for task in program.tasks.keys() {
        let nodes = flatten(program, task)?;
        bases.insert(task.clone(), direct_requirements(&nodes));
    }
    let edges = service_edges(program, &bases);
    prove_service_cycles(&edges)?;
    let required = bases
        .into_iter()
        .map(|(task, base)| (task, close_union(base, &edges)))
        .collect();
    Ok(GraphFacts { required })
}

fn capacity(window: PortWindow, slot: u32, endpoints: usize, services: usize) -> RuntimeResult<()> {
    if endpoints > window.capacity() as usize {
        return Err(RuntimeError::new(
            ErrorCode::ManifestAdmission,
            format!(
                "slot {slot} candidate window {}-{} cannot host {endpoints} endpoints across {services} services",
                window.start(),
                window.end()
            ),
        ));
    }
    Ok(())
}

pub(super) fn prove_capacity(program: &Program, facts: GraphFacts) -> RuntimeResult<()> {
    let demands: BTreeMap<_, _> = facts
        .required
        .into_iter()
        .map(|(task, services)| {
            let endpoints: usize = services
                .iter()
                .map(|id| program.services[id].endpoints.len())
                .sum();
            (task, (endpoints, services.len()))
        })
        .collect();
    for (&slot, &window) in &program.slot_windows {
        for task in program.task_ids() {
            let (endpoints, services) = demands[task];
            capacity(window, slot, endpoints, services)?;
        }
    }
    Ok(())
}

pub fn plan<'a>(
    manifest: &'a ExecutionManifest,
    task: &TaskId,
    slot: u32,
) -> RuntimeResult<RunPlan<'a>> {
    plan_program(&manifest.program, task, slot)
}

fn plan_program<'a>(program: &'a Program, task: &TaskId, slot: u32) -> RuntimeResult<RunPlan<'a>> {
    bind_slot(logical_plan(program, task)?, slot)
}

fn logical_plan<'a>(program: &'a Program, task: &TaskId) -> RuntimeResult<LogicalPlan<'a>> {
    let service_lifetime = match program.tasks.get(task) {
        Some(ExecutableTask::Leaf(task)) => task.service_lifetime,
        Some(ExecutableTask::Composite(task)) => task.service_lifetime,
        None => {
            return Err(RuntimeError::new(
                ErrorCode::ManifestAdmission,
                format!("task {task} is missing"),
            ));
        }
    };
    let nodes = flatten(program, task)?;
    let mut bases = BTreeMap::new();
    for prepare in program
        .services
        .values()
        .filter_map(|service| service.prepare.as_ref())
    {
        if !bases.contains_key(prepare) {
            bases.insert(
                prepare.clone(),
                direct_requirements(&flatten(program, prepare)?),
            );
        }
    }
    let edges = service_edges(program, &bases);
    let required = close_union(direct_requirements(&nodes), &edges);
    let mut remaining: Vec<_> = required.iter().collect();
    let mut services = Vec::with_capacity(remaining.len());
    let mut started = BTreeSet::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .position(|id| edges[*id].iter().all(|(id, _)| started.contains(id)))
            .ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::ManifestAdmission,
                    "service startup dependencies contain a cycle or leave the selected union",
                )
            })?;
        let id = remaining.remove(ready);
        let service = &program.services[id];
        let prepare_nodes = service
            .prepare
            .as_ref()
            .map(|task| flatten(program, task))
            .transpose()?
            .unwrap_or_default();
        services.push((service, prepare_nodes));
        started.insert(id.clone());
    }
    Ok(LogicalPlan {
        service_lifetime,
        services,
        nodes,
        slot_windows: &program.slot_windows,
    })
}

fn bind_slot(logical: LogicalPlan<'_>, slot: u32) -> RuntimeResult<RunPlan<'_>> {
    let window = logical.slot_windows.get(&slot).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ManifestAdmission,
            format!("slot {slot} has no candidate port window"),
        )
    })?;
    let demand = logical
        .services
        .iter()
        .map(|(service, _)| service.endpoints.len())
        .sum();
    capacity(*window, slot, demand, logical.services.len())?;
    // Canonical address allocation is independent of dependency-first startup.
    let mut addresses = BTreeMap::new();
    let canonical: BTreeMap<_, _> = logical
        .services
        .iter()
        .map(|(service, _)| (&service.name, *service))
        .collect();
    let mut cursor = window.start();
    for (id, service) in canonical {
        let endpoints = service
            .endpoints
            .keys()
            .map(|endpoint| {
                let port = cursor;
                cursor = cursor.saturating_add(1);
                (endpoint.clone(), port)
            })
            .collect();
        addresses.insert(id, endpoints);
    }
    let services = logical
        .services
        .into_iter()
        .map(|(service, prepare_nodes)| ServiceBinding {
            service,
            prepare_nodes,
            endpoint_ports: addresses
                .remove(&service.name)
                .expect("every selected service was allocated"),
        })
        .collect();
    Ok(RunPlan {
        service_lifetime: logical.service_lifetime,
        services,
        nodes: logical.nodes,
    })
}

fn flatten<'a>(manifest: &'a Program, root: &TaskId) -> RuntimeResult<Vec<PlanNode<'a>>> {
    struct FlatNode<'a> {
        step_path: String,
        leaf: &'a super::types::ExecTask,
        depends_on: BTreeSet<String>,
    }

    fn emit<'a>(
        manifest: &'a Program,
        task: &TaskId,
        path: String,
        visiting: &mut Vec<TaskId>,
        out: &mut Vec<FlatNode<'a>>,
    ) -> RuntimeResult<()> {
        let composite = match manifest.tasks.get(task) {
            Some(ExecutableTask::Leaf(leaf)) => {
                out.push(FlatNode {
                    step_path: path,
                    leaf,
                    depends_on: BTreeSet::new(),
                });
                return Ok(());
            }
            Some(ExecutableTask::Composite(composite)) => composite,
            None => {
                return Err(RuntimeError::new(
                    ErrorCode::ManifestAdmission,
                    format!("task {task} is missing"),
                ));
            }
        };
        if visiting.contains(task) {
            let chain = visiting
                .iter()
                .map(|id| id.as_str())
                .chain(std::iter::once(task.as_str()))
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(RuntimeError::new(
                ErrorCode::ManifestAdmission,
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
                manifest,
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
        manifest,
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
                ErrorCode::ManifestAdmission,
                format!("task {root} has a step dependency cycle through: {stuck}"),
            ));
        };
        let node = remaining.remove(index);
        placed.insert(node.step_path.clone());
        ordered.push(PlanNode {
            node_id: NodeId::new(&node.step_path),
            task: node.leaf,
        });
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::types::*;
    use std::collections::BTreeMap;
    use std::time::Duration;

    use nixfied_manifest::{
        ContainmentRequirement, OperationId, ServiceId, ServiceLifetime, TaskId,
    };

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
            cwd: RelativeCwd::new(".").unwrap(),
            stdin: StdinPolicy::Null,
            tool_roots: Vec::new(),
        }
    }

    fn tcp_probe() -> ProbePolicy {
        ProbePolicy {
            label: "ready".to_string(),
            timeout: Duration::from_millis(1000),
            retry_interval: Duration::from_millis(100),
            max_attempts: 10.try_into().unwrap(),
        }
    }

    fn service(name: &str) -> ExecService {
        ExecService {
            name: ServiceId::new(name),
            prepare: None,
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
                Endpoint {
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

    /// A manifest whose `all` leaf requires the given services — selecting it
    /// derives exactly that union.
    fn manifest(
        services: Vec<&str>,
        required: Vec<&str>,
        windows: Vec<(u32, u16, u16)>,
    ) -> Program {
        let mut leaf = leaf_task("all");
        leaf.requires = required.into_iter().map(ServiceId::new).collect();
        Program {
            services: services
                .into_iter()
                .map(|name| (ServiceId::new(name), service(name)))
                .collect(),
            tasks: BTreeMap::from([(TaskId::new("all"), ExecutableTask::Leaf(leaf))]),
            slot_windows: windows
                .into_iter()
                .map(|(slot, start, end)| (slot, PortWindow::new(start, end).unwrap()))
                .collect(),
        }
    }

    #[test]
    fn assigns_ports_from_window_start_in_order() {
        let em = manifest(vec!["a", "b"], vec!["a", "b"], vec![(0, 23080, 23090)]);
        let plan = plan_program(&em, &TaskId::new("all"), 0).expect("plan exists");
        assert_eq!(
            plan.services
                .iter()
                .map(|binding| (
                    binding.service.name.as_str(),
                    binding.endpoint_ports.clone()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("a", BTreeMap::from([("e".to_string(), 23080)])),
                ("b", BTreeMap::from([("e".to_string(), 23081)])),
            ]
        );
    }

    #[test]
    fn each_selected_service_gets_its_shared_prepare_task_occurrences() {
        let mut program = manifest(vec!["a", "b"], vec!["a", "b"], vec![(0, 23080, 23090)]);
        program.tasks.insert(
            TaskId::new("prepare"),
            ExecutableTask::Leaf(leaf_task("prepare")),
        );
        for service in program.services.values_mut() {
            service.prepare = Some(TaskId::new("prepare"));
        }
        let plan = plan_program(&program, &TaskId::new("all"), 0).unwrap();
        let occurrences: Vec<_> = plan
            .services
            .iter()
            .flat_map(|binding| {
                binding.prepare_nodes.iter().map(|node| {
                    (
                        binding.service.name.as_str(),
                        node.node_id.as_str(),
                        node.task.task_id.as_str(),
                    )
                })
            })
            .collect();
        assert_eq!(
            occurrences,
            vec![("a", "prepare", "prepare"), ("b", "prepare", "prepare")]
        );
    }

    #[test]
    fn rejects_when_window_cannot_host_all_services() {
        let em = manifest(vec!["a", "b"], vec!["a", "b"], vec![(0, 23080, 23080)]);
        let error = plan_program(&em, &TaskId::new("all"), 0).expect_err("window too small");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
    }

    #[test]
    fn feasibility_is_checked_per_slot() {
        // Slot 0 fits two services; slot 1's window holds only one.
        let em = manifest(
            vec!["a", "b"],
            vec!["a", "b"],
            vec![(0, 23080, 23090), (1, 24000, 24000)],
        );
        assert!(plan_program(&em, &TaskId::new("all"), 0).is_ok());
        assert_eq!(
            plan_program(&em, &TaskId::new("all"), 1)
                .expect_err("slot 1 too small")
                .code,
            ErrorCode::ManifestAdmission
        );
        assert_eq!(
            prove_graph(&em)
                .and_then(|facts| prove_capacity(&em, facts))
                .expect_err("not all slots feasible")
                .code,
            ErrorCode::ManifestAdmission
        );
    }

    fn leaf_task(name: &str) -> ExecTask {
        ExecTask {
            timeout: Some(Duration::from_millis(1000)),
            task_id: TaskId::new(name),
            service_lifetime: ServiceLifetime::RunScoped,
            exec: resolved_exec(),
            requires: Vec::new(),
            success_codes: vec![0],
        }
    }

    fn composite(name: &str, steps: Vec<(&str, &str, Vec<&str>)>) -> ExecComposite {
        ExecComposite {
            task_id: TaskId::new(name),
            service_lifetime: ServiceLifetime::RunScoped,
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

    fn vector_manifest(leaves: Vec<&str>, composites: Vec<ExecComposite>) -> Program {
        let mut em = manifest(vec![], vec![], vec![(0, 23080, 23090)]);
        em.tasks.clear();
        for leaf in leaves {
            em.tasks
                .insert(TaskId::new(leaf), ExecutableTask::Leaf(leaf_task(leaf)));
        }
        for comp in composites {
            em.tasks
                .insert(comp.task_id.clone(), ExecutableTask::Composite(comp));
        }
        em
    }

    fn plan_paths(em: &Program, root: &str) -> Vec<String> {
        plan_program(em, &TaskId::new(root), 0)
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
        let em = vector_manifest(
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
        let em = vector_manifest(
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
        let em = vector_manifest(vec!["fmt"], vec![]);
        assert_eq!(plan_paths(&em, "fmt"), vec!["fmt"]);
    }

    #[test]
    fn selected_task_lifetime_applies_to_service_union() {
        let mut leaf = leaf_task("smoke");
        leaf.service_lifetime = ServiceLifetime::PersistentUntilDown;
        leaf.requires = vec![ServiceId::new("db")];
        let mut composite = composite("stack", vec![("smoke", "smoke", vec![])]);
        composite.service_lifetime = ServiceLifetime::UntilIdle;
        let mut em = manifest(vec!["db"], vec![], vec![(0, 23080, 23090)]);
        em.tasks = BTreeMap::from([
            (TaskId::new("smoke"), ExecutableTask::Leaf(leaf)),
            (TaskId::new("stack"), ExecutableTask::Composite(composite)),
        ]);

        let direct = plan_program(&em, &TaskId::new("smoke"), 0).expect("leaf plan exists");
        assert_eq!(
            direct.service_lifetime,
            ServiceLifetime::PersistentUntilDown
        );
        assert_eq!(
            direct
                .services
                .iter()
                .map(|binding| binding.service.name.as_str())
                .collect::<Vec<_>>(),
            vec!["db"]
        );

        let nested = plan_program(&em, &TaskId::new("stack"), 0).expect("composite plan exists");
        assert_eq!(nested.service_lifetime, ServiceLifetime::UntilIdle);
        assert_eq!(
            nested
                .services
                .iter()
                .map(|binding| binding.service.name.as_str())
                .collect::<Vec<_>>(),
            vec!["db"]
        );
    }

    #[test]
    fn flattening_rejects_a_task_reference_cycle() {
        let em = vector_manifest(
            vec![],
            vec![
                composite("a", vec![("to-b", "b", vec![])]),
                composite("b", vec![("to-a", "a", vec![])]),
            ],
        );
        let error = plan_program(&em, &TaskId::new("a"), 0).expect_err("cycle must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("cycle"), "{}", error.message);
    }

    #[test]
    fn flattening_rejects_a_sibling_depends_on_cycle() {
        let em = vector_manifest(
            vec!["unit"],
            vec![composite(
                "spin",
                vec![("x", "unit", vec!["y"]), ("y", "unit", vec!["x"])],
            )],
        );
        let error = plan_program(&em, &TaskId::new("spin"), 0).expect_err("cycle must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("cycle"), "{}", error.message);
    }

    #[test]
    fn start_order_honors_connects_to_but_ports_stay_declared() {
        // app sorts first (and keeps the first port), but connects to db, so
        // db must start first.
        let mut em = manifest(
            vec!["app", "db"],
            vec!["app", "db"],
            vec![(0, 23080, 23090)],
        );
        em.services
            .get_mut(&ServiceId::new("app"))
            .expect("app exists")
            .connects_to = vec![ServiceId::new("db")];
        let plan = plan_program(&em, &TaskId::new("all"), 0).expect("plan exists");
        assert_eq!(
            plan.services
                .iter()
                .map(|binding| (
                    binding.service.name.as_str(),
                    binding.endpoint_ports.clone()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("db", BTreeMap::from([("e".to_string(), 23081)])),
                ("app", BTreeMap::from([("e".to_string(), 23080)])),
            ]
        );
    }

    #[test]
    fn assigns_a_contiguous_block_per_multi_endpoint_service() {
        // `multi` declares three endpoints, so it consumes a three-port block; the
        // single-endpoint `solo` takes the next port after the block.
        let mut em = manifest(
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
                Endpoint {
                    endpoint_id: id.to_string(),
                    host: LoopbackHost::parse("127.0.0.1").unwrap(),
                },
            );
        }
        multi.endpoints.remove("e");
        multi.primary_endpoint = Some("a".to_string());
        let plan = plan_program(&em, &TaskId::new("all"), 0).expect("plan exists");
        let multi_ports = &plan
            .services
            .iter()
            .find(|b| b.service.name.as_str() == "multi")
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
            .find(|b| b.service.name.as_str() == "solo")
            .expect("solo binding")
            .endpoint_ports;
        assert_eq!(solo_ports, &BTreeMap::from([("e".to_string(), 23083)]));
    }

    #[test]
    fn rejects_when_window_cannot_host_all_endpoints() {
        // One service with two endpoints must reserve both ports atomically.
        let mut em = manifest(vec!["a"], vec!["a"], vec![(0, 23080, 23080)]);
        em.services.get_mut("a").unwrap().endpoints.insert(
            "second".into(),
            Endpoint {
                endpoint_id: "second".into(),
                host: LoopbackHost::parse("127.0.0.1").unwrap(),
            },
        );
        assert_eq!(
            plan_program(&em, &TaskId::new("all"), 0)
                .expect_err("window too small for the endpoint block")
                .code,
            ErrorCode::ManifestAdmission
        );
    }
}
