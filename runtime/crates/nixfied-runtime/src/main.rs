use std::path::{Path, PathBuf};
use std::time::Instant;

use nixfied_model::ServiceLifetime;
use nixfied_runtime::cancellation::{CancellationToken, ProcessSignalGuard};
use nixfied_runtime::execution::plan;
use nixfied_runtime::redaction::Redactor;
use nixfied_runtime::registry::{Registry, RegistryIdentity, RunLeaseHeartbeat};
use nixfied_runtime::service::task::TaskRun;
use nixfied_runtime::service::{
    RunContext, SelectedEndpoint, StartedService, run_dependent_task_cancellable, run_slot_clean,
    start_service_for_slot,
};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{
    StateIdentity, derive_host_placement_for_slot, materialize_registry_root, prepare_slot_state,
    state_base_from_env,
};
use nixfied_runtime::{
    Admission, AdmissionContext, RuntimeError, StoreOriginPolicy, parse_loaded_model,
    read_raw_model,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckOutput {
    model_path: PathBuf,
    computed_model_hash: String,
    raw_len: usize,
    project_id: String,
    runtime_abi: String,
    toolchain_id: String,
    target_system: String,
    environment: String,
    slot: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceRunOutput {
    service_id: String,
    service_instance_id: String,
    process_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_endpoint: Option<SelectedEndpoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeResult {
    node_id: String,
    task_id: String,
    success: bool,
    exit_code: Option<i32>,
    duration_ms: u64,
    // Where the node's full execution detail lives — the task's captured stdout,
    // stderr, and summary — so the summary links each node straight to its
    // evidence without a separate lookup.
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    summary_path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunOutput {
    run_id: String,
    model_path: PathBuf,
    computed_model_hash: String,
    duration_ms: u64,
    services: Vec<ServiceRunOutput>,
    tasks: Vec<TaskRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<TaskRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    nodes: Vec<NodeResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_summary_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeSelection {
    slot: Option<u32>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!(
            "{}",
            serde_json::to_string(&error).unwrap_or_else(|_| error.to_string())
        );
        std::process::exit(exit_code(&error));
    }
}

fn run() -> Result<(), RuntimeError> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map(String::as_str).unwrap_or("check");
    match command {
        "check" => check(args.get(1..).unwrap_or(&[])),
        "run" => run_m0(args.get(1..).unwrap_or(&[])),
        "ps" => run_control(ControlCommand::Ps, args.get(1..).unwrap_or(&[])),
        "down" => run_control(ControlCommand::Down, args.get(1..).unwrap_or(&[])),
        "clean" => run_control(ControlCommand::Clean, args.get(1..).unwrap_or(&[])),
        _ => Err(RuntimeError::unsupported_feature(
            "runtime.command",
            format!("unsupported runtime command: {command}"),
        )
        .with_detail("command", command)),
    }
}

fn check(args: &[String]) -> Result<(), RuntimeError> {
    let mut model_path = None;
    let mut allow_non_store = false;
    let mut slot = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--model" => {
                index += 1;
                model_path = args.get(index).map(PathBuf::from);
            }
            "--allow-non-store-model" => {
                allow_non_store = true;
            }
            "--slot" => {
                index += 1;
                slot = Some(parse_slot_arg(args.get(index), "--slot")?);
            }
            other => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::ModelAdmission,
                    format!("unknown check argument: {other}"),
                ));
            }
        }
        index += 1;
    }
    let model_path = model_path.ok_or_else(|| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            "missing --model path",
        )
    })?;
    let (loaded, admission) = load_admitted_model(model_path, allow_non_store)?;
    let selected_slot = select_slot(&loaded.model, slot)?;
    let output = CheckOutput {
        model_path: admission.model_path,
        computed_model_hash: admission.computed_model_hash,
        raw_len: admission.raw_len,
        project_id: admission.project_id,
        runtime_abi: admission.runtime_abi,
        toolchain_id: admission.toolchain_id,
        target_system: admission.target_system,
        environment: selected_slot.environment.to_string(),
        slot: selected_slot.slot,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("check output should serialize")
    );
    Ok(())
}

fn run_m0(args: &[String]) -> Result<(), RuntimeError> {
    let options = parse_run_options(args)?;
    let _signals = ProcessSignalGuard::install()?;
    let cancellation = CancellationToken::new();
    let run_id = new_run_id();
    let (loaded, admission) =
        load_admitted_model(options.model_path.clone(), options.allow_non_store)?;
    let redactor = Redactor::from_secrets(&admission.secrets);
    let model_path = admission.model_path.clone();
    let computed_model_hash = admission.computed_model_hash.clone();
    let output = run_m0_admitted(
        &loaded.model,
        &admission,
        &redactor,
        &options,
        run_id,
        &cancellation,
    )
    .map_err(|error| {
        redactor.redact_error(error.with_model_if_missing(model_path, computed_model_hash))
    })?;
    print_json_redacted(&output, &redactor)
}

fn run_m0_admitted(
    model: &nixfied_model::Model,
    admission: &Admission,
    redactor: &Redactor,
    options: &RunOptions,
    run_id: String,
    cancellation: &CancellationToken,
) -> Result<RunOutput, RuntimeError> {
    cancellation.check()?;
    let selected_slot = select_slot(model, options.selection.slot)?;
    let placement =
        derive_host_placement_for_slot(model, &selected_slot, &run_id, &options.state_base)?;
    // Every failure past this point carries the run's identity and state paths:
    // the operator must be able to find the evidence without re-deriving the
    // placement by hand.
    run_m0_placed(
        model,
        admission,
        redactor,
        options,
        &run_id,
        &selected_slot,
        &placement,
        cancellation,
    )
    .map_err(|error| enrich_run_error(error, &run_id, &placement))
}

/// Attach the run identity and state paths to a run error, preserving any
/// details the failure site already recorded.
fn enrich_run_error(
    error: RuntimeError,
    run_id: &str,
    placement: &nixfied_runtime::state::HostPlacement,
) -> RuntimeError {
    error
        .with_detail("runId", run_id)
        .with_detail("stateRoot", &placement.state_root)
        .with_detail("runDir", &placement.run_dir)
        .with_detail("logsDir", &placement.logs_dir)
}

#[allow(clippy::too_many_arguments)]
fn run_m0_placed(
    model: &nixfied_model::Model,
    admission: &Admission,
    redactor: &Redactor,
    options: &RunOptions,
    run_id: &str,
    selected_slot: &nixfied_runtime::slot::SelectedSlot<'_>,
    placement: &nixfied_runtime::state::HostPlacement,
    cancellation: &CancellationToken,
) -> Result<RunOutput, RuntimeError> {
    let run_started = Instant::now();
    // The registry opens before the marker decision: when the slot was last
    // used by a different model build, the upgrade path needs registry evidence
    // to tear down what that build left running.
    materialize_registry_root(placement)?;
    let identity = StateIdentity::from_selected_slot(model, admission, selected_slot);
    let mut registry = Registry::open_or_create(
        placement.registry_path(),
        &RegistryIdentity::for_slot(
            &model.project.project_id,
            selected_slot.environment,
            selected_slot.slot,
            &model.runtime_abi,
            &model.toolchain_id,
        ),
    )?;
    registry.set_redactor(redactor.clone());
    let _ = nixfied_runtime::control::reconcile_registry(&mut registry)?;
    let upgrade = prepare_slot_state(placement, &identity, &mut registry, options.timeout_ms)?;
    if upgrade.upgraded {
        eprintln!(
            "  upgraded slot state from model {} (state {})",
            upgrade.from_model_hash.as_deref().unwrap_or("unknown"),
            if upgrade.cleaned {
                "cleaned: state epoch changed"
            } else {
                "preserved"
            }
        );
    }

    // A run drives one selected task: its flattened nodes plus the derived
    // service union, started eagerly. The plan (service ports + node order) is
    // a pure function of the lowered model, the task, and the slot, already
    // proven feasible at admission. `run` with no selection refuses and lists
    // the declared tasks — there is no implicit default.
    let selected_task = match options.task.as_deref() {
        Some(task) => nixfied_model::TaskId::new(task),
        None => {
            return Err(selection_required_error(&admission.execution_model));
        }
    };
    if !admission
        .execution_model
        .tasks
        .contains_key(selected_task.as_str())
        && !admission
            .execution_model
            .composites
            .contains_key(selected_task.as_str())
    {
        return Err(selection_required_error(&admission.execution_model)
            .with_detail("unknownTask", selected_task.as_str()));
    }
    let plan = plan(
        &admission.execution_model,
        &selected_task,
        selected_slot.slot,
    )?;

    // Record the run row before any service starts, so even a service-less
    // selection (a task tree whose leaves require nothing) leaves durable
    // run evidence for `ps`/reconcile. `INSERT OR IGNORE` makes the service-path
    // run-row insert a harmless no-op.
    nixfied_runtime::service::registry::record_run_created(
        &mut registry,
        run_id,
        admission,
        placement,
    )?;

    // The slot's cross-service endpoint map, known deterministically before
    // anything spawns: `${port:<serviceId>}` substitution addresses each service's
    // primary endpoint by service id.
    let slot_endpoints: nixfied_runtime::service::SlotEndpoints = plan
        .services
        .iter()
        .filter_map(|binding| {
            let service = admission
                .execution_model
                .services
                .get(binding.service_name.as_str())?;
            let primary_id = service.primary_endpoint.as_ref()?;
            let primary = service.endpoints.get(primary_id)?;
            let port = *binding.endpoint_ports.get(primary_id)?;
            Some((
                binding.service_name.clone(),
                nixfied_runtime::service::SelectedEndpoint {
                    endpoint_id: primary.endpoint_id.clone(),
                    host: primary.host.to_string(),
                    port,
                },
            ))
        })
        .collect();

    // Start each required service on its planned port block, waiting readiness
    // then health before the next.
    let mut started: Vec<StartedService> = Vec::new();
    let mut lease: Option<RunLeaseHeartbeat> = None;
    let source_root = admission.require_source()?.observed_root.clone();
    for binding in &plan.services {
        let service_name = binding.service_name.as_str();
        eprintln!("  starting service {service_name}");

        // prepare-as-task: the runner executes the prepare task's flattened
        // nodes inside the service reservation, resolving each leaf's
        // requirements against the services already started (the combined
        // connectsTo + prepare-requires ordering guarantees they are ready).
        let service_def = admission
            .execution_model
            .services
            .get(service_name)
            .ok_or_else(|| {
                RuntimeError::new(
                    nixfied_runtime::ErrorCode::ModelAdmission,
                    format!("service {service_name} is missing"),
                )
            })?;
        let prepare_runner: Option<nixfied_runtime::service::process::PrepareRunner<'_>> =
            service_def.prepare.clone().map(|prepare_task| {
                let started_services = &started;
                let source_root = source_root.clone();
                Box::new(move |registry: &mut Registry| -> Result<(), RuntimeError> {
                    let nodes = nixfied_runtime::execution::flatten_task(
                        &admission.execution_model,
                        &prepare_task,
                    )?;
                    for node in nodes {
                        let task = admission
                            .execution_model
                            .tasks
                            .get(node.task_id.as_str())
                            .ok_or_else(|| {
                                RuntimeError::new(
                                    nixfied_runtime::ErrorCode::ModelAdmission,
                                    format!("task {} is missing", node.task_id),
                                )
                            })?;
                        let mut dependencies: Vec<&StartedService> = Vec::new();
                        for name in &task.requires {
                            let Some(dependency) = started_services
                                .iter()
                                .find(|service| service.service_name() == name.as_str())
                            else {
                                return Err(RuntimeError::new(
                                    nixfied_runtime::ErrorCode::DependencyUnavailable,
                                    format!(
                                        "prepare node {} requires service {name} which is not started yet",
                                        node.node_id
                                    ),
                                ));
                            };
                            dependencies.push(dependency);
                        }
                        eprintln!("  prepare node {} ({})", node.node_id, node.task_id);
                        run_dependent_task_cancellable(
                            placement,
                            registry,
                            RunContext {
                                run_id,
                                computed_model_hash: &admission.computed_model_hash,
                                source_root: &source_root,
                                state_root: &placement.state_root,
                                secrets: &admission.secrets,
                                redactor,
                            },
                            &dependencies,
                            node.node_id.as_str(),
                            task,
                            cancellation,
                        )?;
                    }
                    Ok(())
                }) as nixfied_runtime::service::process::PrepareRunner<'_>
            });

        let started_service = match start_service_for_slot(
            admission,
            placement,
            &mut registry,
            run_id,
            selected_slot,
            nixfied_runtime::service::process::ServiceSelection {
                service_name,
                service_lifetime: plan.service_lifetime,
                endpoint_ports: &binding.endpoint_ports,
                slot_endpoints: &slot_endpoints,
                prepare_runner,
            },
        ) {
            Ok(service) => service,
            Err(error) => {
                let duration_ms = elapsed_ms(run_started);
                let summary = write_failure_run_summary(
                    placement,
                    run_id,
                    duration_ms,
                    &[],
                    &started,
                    &[],
                    redactor,
                );
                print_run_footer(
                    false,
                    &[],
                    duration_ms,
                    summary.as_deref(),
                    &placement.logs_dir,
                );
                teardown(
                    &mut started,
                    &mut registry,
                    options.timeout_ms,
                    cancellation.is_canceled(),
                );
                stop_lease(lease)?;
                return Err(with_failure_summary(
                    error.with_detail("failedService", service_name),
                    summary,
                ));
            }
        };
        if lease.is_none() {
            lease = Some(RunLeaseHeartbeat::start(
                placement.registry_path().to_path_buf(),
                registry.identity().clone(),
                started_service.run_id.clone(),
                started_service.owner_token.clone(),
            ));
        }
        started.push(started_service);

        let service = started.last_mut().expect("just pushed a service");
        if let Err(error) = service.wait_for_probe_ready_cancellable(&mut registry, cancellation) {
            let duration_ms = elapsed_ms(run_started);
            let summary = write_failure_run_summary(
                placement,
                run_id,
                duration_ms,
                &[],
                &started,
                &[],
                redactor,
            );
            print_run_footer(
                false,
                &[],
                duration_ms,
                summary.as_deref(),
                &placement.logs_dir,
            );
            teardown(
                &mut started,
                &mut registry,
                options.timeout_ms,
                error.code == nixfied_runtime::ErrorCode::Canceled,
            );
            stop_lease(lease)?;
            return Err(with_failure_summary(
                error.with_detail("failedService", service_name),
                summary,
            ));
        }
        let service = started.last_mut().expect("just pushed a service");
        if let Err(error) = service.check_health_cancellable(&mut registry, cancellation) {
            let duration_ms = elapsed_ms(run_started);
            let summary = write_failure_run_summary(
                placement,
                run_id,
                duration_ms,
                &[],
                &started,
                &[],
                redactor,
            );
            print_run_footer(
                false,
                &[],
                duration_ms,
                summary.as_deref(),
                &placement.logs_dir,
            );
            teardown(
                &mut started,
                &mut registry,
                options.timeout_ms,
                error.code == nixfied_runtime::ErrorCode::Canceled,
            );
            stop_lease(lease)?;
            return Err(with_failure_summary(
                error.with_detail("failedService", service_name),
                summary,
            ));
        }
        let service = started.last().expect("just started a service");
        match &service.selected_endpoint {
            Some(endpoint) => eprintln!(
                "  service {} ready at {}:{}",
                service.service_name(),
                endpoint.host,
                endpoint.port
            ),
            None => eprintln!("  service {} ready (endpoint-less)", service.service_name()),
        }
    }

    let mut task_runs: Vec<TaskRun> = Vec::new();
    let mut node_results: Vec<NodeResult> = Vec::new();
    if let Err(error) = cancellation.check() {
        let duration_ms = elapsed_ms(run_started);
        let summary = write_failure_run_summary(
            placement,
            run_id,
            duration_ms,
            &node_results,
            &started,
            &task_runs,
            redactor,
        );
        print_run_footer(
            false,
            &node_results,
            duration_ms,
            summary.as_deref(),
            &placement.logs_dir,
        );
        teardown(&mut started, &mut registry, options.timeout_ms, true);
        stop_lease(lease)?;
        return Err(with_failure_summary(error, summary));
    }

    // Run each flattened node in dependency order, gating each leaf on the
    // readiness of its declared service requirements (the first provides
    // ${port}/${host} substitution). The plan's order already honors the
    // composite's step dependencies.
    for node in &plan.nodes {
        let task_id = &node.task_id;
        let task = admission
            .execution_model
            .tasks
            .get(task_id.as_str())
            .ok_or_else(|| {
                RuntimeError::new(
                    nixfied_runtime::ErrorCode::ModelAdmission,
                    format!("task {task_id} is missing"),
                )
            })?;
        // Resolve every service this task depends on to its started instance (the
        // first is the primary, providing ${port}/${host}). A task may declare
        // zero services — it runs in the run context alone.
        let mut dep_indices = Vec::new();
        let mut missing_dependency = None;
        for name in &task.requires {
            match started
                .iter()
                .position(|service| service.service_name() == name.as_str())
            {
                Some(index) => dep_indices.push(index),
                None => {
                    missing_dependency = Some(name.clone());
                    break;
                }
            }
        }
        if let Some(name) = missing_dependency {
            let error = RuntimeError::new(
                nixfied_runtime::ErrorCode::DependencyUnavailable,
                format!("task {task_id} depends on service {name} which was not started"),
            )
            .with_detail("failedNodeId", node.node_id.as_str());
            let duration_ms = elapsed_ms(run_started);
            let summary = write_failure_run_summary(
                placement,
                run_id,
                duration_ms,
                &node_results,
                &started,
                &task_runs,
                redactor,
            );
            print_run_footer(
                false,
                &node_results,
                duration_ms,
                summary.as_deref(),
                &placement.logs_dir,
            );
            teardown(&mut started, &mut registry, options.timeout_ms, false);
            stop_lease(lease)?;
            return Err(with_failure_summary(error, summary));
        }
        let dependencies: Vec<&StartedService> =
            dep_indices.iter().map(|&index| &started[index]).collect();
        let run_context = RunContext {
            run_id,
            computed_model_hash: &admission.computed_model_hash,
            source_root: &source_root,
            state_root: &placement.state_root,
            secrets: &admission.secrets,
            redactor,
        };
        let task_result = run_dependent_task_cancellable(
            placement,
            &mut registry,
            run_context,
            &dependencies,
            node.node_id.as_str(),
            task,
            cancellation,
        );
        match task_result {
            Ok(task_run) => {
                eprintln!(
                    "  ok {} ({task_id}) {}",
                    node.node_id,
                    human_duration(task_run.duration_ms)
                );
                node_results.push(NodeResult {
                    node_id: node.node_id.as_str().to_string(),
                    task_id: task_id.as_str().to_string(),
                    success: task_run.success,
                    exit_code: task_run.exit_code,
                    duration_ms: task_run.duration_ms,
                    stdout_path: task_run.stdout_path.clone(),
                    stderr_path: task_run.stderr_path.clone(),
                    summary_path: task_run.summary_path.clone(),
                });
                task_runs.push(task_run);
            }
            Err(error) => {
                // The failed task's evidence rides on the error (see
                // run_dependent_task_cancellable); fold it into the node
                // results so the failure summary records the failed node
                // alongside the completed ones.
                let mut error = error.with_detail("failedNodeId", node.node_id.as_str());
                let failed_run = error
                    .details
                    .get("taskRun")
                    .and_then(|value| serde_json::from_value::<TaskRun>(value.clone()).ok());
                if let Some(task_run) = failed_run {
                    eprintln!(
                        "  fail {} ({task_id}) {} exit={}",
                        node.node_id,
                        human_duration(task_run.duration_ms),
                        human_exit_code(task_run.exit_code)
                    );
                    eprintln!("    stderr: {}", human_path(&task_run.stderr_path));
                    error = error
                        .with_detail("stdoutPath", &task_run.stdout_path)
                        .with_detail("stderrPath", &task_run.stderr_path)
                        .with_detail("summaryPath", &task_run.summary_path);
                    node_results.push(NodeResult {
                        node_id: node.node_id.as_str().to_string(),
                        task_id: task_id.as_str().to_string(),
                        success: task_run.success,
                        exit_code: task_run.exit_code,
                        duration_ms: task_run.duration_ms,
                        stdout_path: task_run.stdout_path.clone(),
                        stderr_path: task_run.stderr_path.clone(),
                        summary_path: task_run.summary_path.clone(),
                    });
                    task_runs.push(task_run);
                } else {
                    eprintln!("  fail {} ({task_id})", node.node_id);
                }
                let duration_ms = elapsed_ms(run_started);
                let summary = write_failure_run_summary(
                    placement,
                    run_id,
                    duration_ms,
                    &node_results,
                    &started,
                    &task_runs,
                    redactor,
                );
                print_run_footer(
                    false,
                    &node_results,
                    duration_ms,
                    summary.as_deref(),
                    &placement.logs_dir,
                );
                teardown(
                    &mut started,
                    &mut registry,
                    options.timeout_ms,
                    error.code == nixfied_runtime::ErrorCode::Canceled,
                );
                stop_lease(lease)?;
                return Err(with_failure_summary(error, summary));
            }
        }
    }

    // Built before the summary so the run record captures the live services
    // (endpoints, instance ids) alongside the node and task results.
    let services_output = services_output(&started);

    if cancellation.is_canceled() {
        let duration_ms = elapsed_ms(run_started);
        let summary = write_failure_run_summary_from_services(
            placement,
            run_id,
            duration_ms,
            &node_results,
            &services_output,
            &task_runs,
            redactor,
        );
        print_run_footer(
            false,
            &node_results,
            duration_ms,
            summary.as_deref(),
            &placement.logs_dir,
        );
        teardown(&mut started, &mut registry, options.timeout_ms, true);
        stop_lease(lease)?;
        return Err(with_failure_summary(
            nixfied_runtime::cancellation::canceled_error(),
            summary,
        ));
    }
    // Stop services in reverse start order. On a stop error, tear down the
    // remaining services instead of aborting the loop: leaving them to Drop
    // would kill the processes without updating registry rows, leases, or
    // port reservations, blocking later clean/runs on the slot.
    while let Some(service) = started.pop() {
        let result = if matches!(plan.service_lifetime, ServiceLifetime::RunScoped) {
            service.stop_cancellable(&mut registry, options.timeout_ms, cancellation)
        } else {
            service.stand(&mut registry)
        };
        if let Err(error) = result {
            let duration_ms = elapsed_ms(run_started);
            let summary = write_failure_run_summary_from_services(
                placement,
                run_id,
                duration_ms,
                &node_results,
                &services_output,
                &task_runs,
                redactor,
            );
            print_run_footer(
                false,
                &node_results,
                duration_ms,
                summary.as_deref(),
                &placement.logs_dir,
            );
            teardown(
                &mut started,
                &mut registry,
                options.timeout_ms,
                error.code == nixfied_runtime::ErrorCode::Canceled,
            );
            stop_lease(lease)?;
            return Err(with_failure_summary(error, summary));
        }
    }
    // Settle a run that no service stop and no task finalized (a degenerate
    // selection with no services and no tasks). Guarded on `service-starting`, so
    // a service- or task-derived terminal status is left untouched.
    nixfied_runtime::service::registry::mark_run_completed(&mut registry, run_id)?;
    stop_lease(lease)?;
    let duration_ms = elapsed_ms(run_started);
    let run_summary_path = Some(write_run_summary(
        placement,
        run_id,
        true,
        duration_ms,
        &node_results,
        &services_output,
        &task_runs,
        redactor,
    )?);
    print_run_footer(
        true,
        &node_results,
        duration_ms,
        run_summary_path.as_deref(),
        &placement.logs_dir,
    );
    let primary_task = task_runs.last().cloned();
    let output = RunOutput {
        run_id: run_id.to_string(),
        model_path: admission.model_path.clone(),
        computed_model_hash: admission.computed_model_hash.clone(),
        duration_ms,
        services: services_output,
        summary_path: primary_task.as_ref().map(|task| task.summary_path.clone()),
        task: primary_task,
        tasks: task_runs,
        nodes: node_results,
        run_summary_path,
    };
    Ok(output)
}

fn services_output(started: &[StartedService]) -> Vec<ServiceRunOutput> {
    started
        .iter()
        .map(|service| ServiceRunOutput {
            service_id: service.service_name().to_string(),
            service_instance_id: service.service_instance_id.clone(),
            process_key: service.process_key.clone(),
            selected_endpoint: service.selected_endpoint.clone(),
        })
        .collect()
}

/// Write the run summary for a failed run — the nodes completed so far plus
/// the failed node — so a failure leaves the same aggregate evidence a success
/// does. Returns the path, or `None` when the summary itself could not be
/// written (the original failure must surface either way).
fn write_failure_run_summary(
    placement: &nixfied_runtime::state::HostPlacement,
    run_id: &str,
    duration_ms: u64,
    nodes: &[NodeResult],
    started: &[StartedService],
    tasks: &[TaskRun],
    redactor: &Redactor,
) -> Option<PathBuf> {
    let services = services_output(started);
    write_failure_run_summary_from_services(
        placement,
        run_id,
        duration_ms,
        nodes,
        &services,
        tasks,
        redactor,
    )
}

fn write_failure_run_summary_from_services(
    placement: &nixfied_runtime::state::HostPlacement,
    run_id: &str,
    duration_ms: u64,
    nodes: &[NodeResult],
    services: &[ServiceRunOutput],
    tasks: &[TaskRun],
    redactor: &Redactor,
) -> Option<PathBuf> {
    // The run is failing regardless of what the recorded nodes say — a service
    // or spawn failure can leave zero failed nodes, which must not read as
    // success.
    write_run_summary(
        placement,
        run_id,
        false,
        duration_ms,
        nodes,
        services,
        tasks,
        redactor,
    )
    .ok()
}

fn with_failure_summary(error: RuntimeError, summary_path: Option<PathBuf>) -> RuntimeError {
    match summary_path {
        Some(path) => error.with_detail("runSummaryPath", &path),
        None => error,
    }
}

/// Tear down already-started services in reverse order on a run error, either
/// cancelling (process-group cancellation, recorded as canceled) or stopping.
fn teardown(
    started: &mut Vec<StartedService>,
    registry: &mut Registry,
    timeout_ms: u64,
    canceled: bool,
) {
    while let Some(mut service) = started.pop() {
        if canceled {
            let _ = service.cancel(registry, timeout_ms, "run canceled");
        } else {
            let _ = service.stop(registry, timeout_ms);
        }
    }
}

fn stop_lease(lease: Option<RunLeaseHeartbeat>) -> Result<(), RuntimeError> {
    if let Some(lease) = lease {
        lease.stop()?;
    }
    Ok(())
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn human_duration(duration_ms: u64) -> String {
    if duration_ms < 1000 {
        format!("{duration_ms}ms")
    } else {
        format!("{:.2}s", duration_ms as f64 / 1000.0)
    }
}

fn human_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('{', "(")
        .replace('}', ")")
}

fn human_exit_code(exit_code: Option<i32>) -> String {
    exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn print_run_footer(
    run_succeeded: bool,
    nodes: &[NodeResult],
    duration_ms: u64,
    run_summary_path: Option<&Path>,
    logs_dir: &Path,
) {
    if run_succeeded {
        eprintln!(
            "  result: ok {} passed, 0 failed in {}",
            nodes.iter().filter(|node| node.success).count(),
            human_duration(duration_ms)
        );
    } else if nodes.is_empty() {
        eprintln!("  result: fail in {}", human_duration(duration_ms));
    } else {
        eprintln!(
            "  result: fail {} passed, {} failed in {}",
            nodes.iter().filter(|node| node.success).count(),
            nodes.iter().filter(|node| !node.success).count(),
            human_duration(duration_ms)
        );
    }
    if let Some(path) = run_summary_path {
        eprintln!("  run-summary: {}", human_path(path));
    }
    eprintln!("  logs: {}", human_path(logs_dir));
}

/// Write the aggregate run summary: the run id, overall success, the services
/// started (with their resolved endpoints), and the per-node and per-task
/// results — a complete, inspectable record of the run.
fn write_run_summary(
    placement: &nixfied_runtime::state::HostPlacement,
    run_id: &str,
    run_succeeded: bool,
    duration_ms: u64,
    nodes: &[NodeResult],
    services: &[ServiceRunOutput],
    tasks: &[TaskRun],
    redactor: &Redactor,
) -> Result<PathBuf, RuntimeError> {
    let path = placement.artifacts_dir.join("run-summary.json");
    let mut summary = serde_json::json!({
        "runId": run_id,
        "success": run_succeeded && nodes.iter().all(|node| node.success),
        "durationMs": duration_ms,
        "services": services,
        "nodes": nodes,
        "tasks": tasks,
    });
    redactor.redact_value(&mut summary);
    let bytes = serde_json::to_vec_pretty(&summary).map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            error.to_string(),
        )
    })?;
    std::fs::write(&path, bytes).map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::StateUnwritable,
            format!("failed to write run summary {}: {error}", path.display()),
        )
    })?;
    Ok(path)
}

#[derive(Debug, Clone, Copy)]
enum ControlCommand {
    Ps,
    Down,
    Clean,
}

struct ControlOptions {
    model_path: PathBuf,
    allow_non_store: bool,
    state_base: PathBuf,
    timeout_ms: u64,
    selection: RuntimeSelection,
    cleanup_mode: nixfied_runtime::state::CleanupMode,
}

struct RunOptions {
    model_path: PathBuf,
    allow_non_store: bool,
    state_base: PathBuf,
    timeout_ms: u64,
    selection: RuntimeSelection,
    task: Option<String>,
}

/// The refusal for a missing or unknown task selection: name every declared
/// task so the operator can pick one.
fn selection_required_error(model: &nixfied_runtime::execution::ExecutionModel) -> RuntimeError {
    let declared: Vec<&str> = model
        .tasks
        .keys()
        .chain(model.composites.keys())
        .map(|task| task.as_str())
        .collect();
    RuntimeError::new(
        nixfied_runtime::ErrorCode::ModelAdmission,
        format!(
            "run requires --task <id>; declared tasks: {}",
            declared.join(", ")
        ),
    )
    .with_detail("declaredTasks", &declared)
}

fn run_control(command: ControlCommand, args: &[String]) -> Result<(), RuntimeError> {
    let options = parse_control_options(command, args)?;
    let (loaded, admission) =
        load_admitted_model_for_control(options.model_path.clone(), options.allow_non_store)?;
    let model_path = admission.model_path.clone();
    let computed_model_hash = admission.computed_model_hash.clone();
    run_control_admitted(command, &loaded.model, &admission, &options)
        .map_err(|error| error.with_model_if_missing(model_path, computed_model_hash))
}

fn run_control_admitted(
    command: ControlCommand,
    model: &nixfied_model::Model,
    admission: &Admission,
    options: &ControlOptions,
) -> Result<(), RuntimeError> {
    let selected_slot = select_slot(model, options.selection.slot)?;
    let placement =
        derive_host_placement_for_slot(model, &selected_slot, "control", &options.state_base)?;
    let mut registry = Registry::open_or_create(
        placement.registry_path(),
        &RegistryIdentity::for_slot(
            &model.project.project_id,
            selected_slot.environment,
            selected_slot.slot,
            &model.runtime_abi,
            &model.toolchain_id,
        ),
    )?;
    match command {
        ControlCommand::Ps => print_json(&nixfied_runtime::control::ps(&mut registry)?),
        ControlCommand::Down => print_json(&nixfied_runtime::control::down_owned_process_groups(
            &mut registry,
            options.timeout_ms,
        )?),
        ControlCommand::Clean => print_json(&run_slot_clean(
            model,
            admission,
            &placement,
            &mut registry,
            &selected_slot,
            options.cleanup_mode,
        )?),
    }
}

fn parse_run_options(args: &[String]) -> Result<RunOptions, RuntimeError> {
    let mut model_path = None;
    let mut allow_non_store = false;
    let mut state_base = None;
    let mut timeout_ms = 5000;
    let mut slot = None;
    let mut task = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--model" => {
                index += 1;
                model_path = args.get(index).map(PathBuf::from);
            }
            "--allow-non-store-model" => {
                allow_non_store = true;
            }
            "--state-base" => {
                index += 1;
                state_base = args.get(index).map(PathBuf::from);
            }
            "--slot" => {
                index += 1;
                slot = Some(parse_slot_arg(args.get(index), "--slot")?);
            }
            "--task" => {
                index += 1;
                task = Some(
                    args.get(index)
                        .ok_or_else(|| {
                            RuntimeError::new(
                                nixfied_runtime::ErrorCode::ModelAdmission,
                                "missing --task value",
                            )
                        })?
                        .clone(),
                );
            }
            "--timeout-ms" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::ModelAdmission,
                        "missing --timeout-ms value",
                    )
                })?;
                timeout_ms = value.parse::<u64>().map_err(|error| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::ModelAdmission,
                        format!("invalid --timeout-ms value {value}: {error}"),
                    )
                })?;
            }
            other => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::ModelAdmission,
                    format!("unknown run argument: {other}"),
                ));
            }
        }
        index += 1;
    }
    let model_path = model_path.ok_or_else(|| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            "missing --model path",
        )
    })?;
    let state_base = state_base.map(Ok).unwrap_or_else(state_base_from_env)?;
    Ok(RunOptions {
        model_path,
        allow_non_store,
        state_base,
        timeout_ms,
        selection: RuntimeSelection { slot },
        task,
    })
}

fn parse_control_options(
    command: ControlCommand,
    args: &[String],
) -> Result<ControlOptions, RuntimeError> {
    let mut model_path = None;
    let mut allow_non_store = false;
    let mut state_base = None;
    let mut timeout_ms = 5000;
    let mut slot = None;
    let mut cleanup_mode = nixfied_runtime::state::CleanupMode::Standard;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--model" => {
                index += 1;
                model_path = args.get(index).map(PathBuf::from);
            }
            "--allow-non-store-model" => {
                allow_non_store = true;
            }
            "--state-base" => {
                index += 1;
                state_base = args.get(index).map(PathBuf::from);
            }
            "--slot" => {
                index += 1;
                slot = Some(parse_slot_arg(args.get(index), "--slot")?);
            }
            "--purge" if matches!(command, ControlCommand::Clean) => {
                cleanup_mode = nixfied_runtime::state::CleanupMode::Purge;
            }
            "--timeout-ms" if matches!(command, ControlCommand::Down) => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::ModelAdmission,
                        "missing --timeout-ms value",
                    )
                })?;
                timeout_ms = value.parse::<u64>().map_err(|error| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::ModelAdmission,
                        format!("invalid --timeout-ms value {value}: {error}"),
                    )
                })?;
            }
            other => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::ModelAdmission,
                    format!("unknown control argument: {other}"),
                ));
            }
        }
        index += 1;
    }
    let model_path = model_path.ok_or_else(|| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            "missing --model path",
        )
    })?;
    let state_base = state_base.map(Ok).unwrap_or_else(state_base_from_env)?;
    Ok(ControlOptions {
        model_path,
        allow_non_store,
        state_base,
        timeout_ms,
        selection: RuntimeSelection { slot },
        cleanup_mode,
    })
}

fn parse_slot_arg(value: Option<&String>, flag: &str) -> Result<u32, RuntimeError> {
    let value = value.ok_or_else(|| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            format!("missing {flag} value"),
        )
    })?;
    value.parse::<u32>().map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            format!("invalid {flag} value {value}: {error}"),
        )
    })
}

fn new_run_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("run-{}-{now}", std::process::id())
}

fn load_admitted_model(
    model_path: PathBuf,
    allow_non_store: bool,
) -> Result<(nixfied_runtime::model_loader::LoadedModel, Admission), RuntimeError> {
    load_model_admitted(model_path, allow_non_store, true)
}

/// Load and admit a model for a recovery/control command (`ps`/`down`/`clean`)
/// without resolving the live workspace, so control can reconcile, stop, and
/// clean a slot from the store model and registry even when run outside the
/// project root or after the workspace has moved or been deleted.
fn load_admitted_model_for_control(
    model_path: PathBuf,
    allow_non_store: bool,
) -> Result<(nixfied_runtime::model_loader::LoadedModel, Admission), RuntimeError> {
    load_model_admitted(model_path, allow_non_store, false)
}

fn load_model_admitted(
    model_path: PathBuf,
    allow_non_store: bool,
    resolve_source: bool,
) -> Result<(nixfied_runtime::model_loader::LoadedModel, Admission), RuntimeError> {
    let policy = if allow_non_store {
        StoreOriginPolicy::AllowNonStoreForTests
    } else {
        StoreOriginPolicy::RequireStore
    };
    let context = AdmissionContext::current(policy);
    let raw_model = read_raw_model(&model_path)?;
    nixfied_runtime::admission::origin::check_raw_store_origin(&raw_model, &context)?;
    let loaded = parse_loaded_model(raw_model)?;
    let admission = if resolve_source {
        Admission::check(&loaded, &context)?
    } else {
        Admission::check_for_control(&loaded, &context)?
    };
    warn_on_ephemeral_port_overlap(&loaded.model);
    Ok((loaded, admission))
}

/// Warn (stderr, non-fatal) when a slot's candidate port window overlaps the
/// host's ephemeral port range: the kernel hands out ports in that range to
/// any process, so a deterministic window inside it can collide with unrelated
/// ephemeral allocations. The range is host state only Linux exposes a stable
/// path for; elsewhere the check is silently skipped.
fn warn_on_ephemeral_port_overlap(model: &nixfied_model::Model) {
    let Some((low, high)) = host_ephemeral_port_range() else {
        return;
    };
    for placement in model.placement.slot_placements.values() {
        let window = &placement.candidate_ports;
        if u32::from(window.start) <= high && u32::from(window.end) >= low {
            eprintln!(
                "warning: slot {} candidate port window {}-{} overlaps the host ephemeral port range {low}-{high}; deterministic ports may collide with ephemeral allocations (set nixfied.placement.ports.base outside the range)",
                placement.slot, window.start, window.end
            );
        }
    }
}

fn host_ephemeral_port_range() -> Option<(u32, u32)> {
    let contents = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range").ok()?;
    let mut parts = contents.split_whitespace();
    let low = parts.next()?.parse().ok()?;
    let high = parts.next()?.parse().ok()?;
    (low <= high).then_some((low, high))
}

fn print_json(value: &impl Serialize) -> Result<(), RuntimeError> {
    print_json_redacted(value, &Redactor::empty())
}

fn print_json_redacted(value: &impl Serialize, redactor: &Redactor) -> Result<(), RuntimeError> {
    let mut value = serde_json::to_value(value).map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            error.to_string(),
        )
    })?;
    redactor.redact_value(&mut value);
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|error| RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            error.to_string()
        ))?
    );
    Ok(())
}

fn exit_code(error: &RuntimeError) -> i32 {
    match error.code {
        nixfied_runtime::ErrorCode::RuntimeAbiMismatch => 12,
        nixfied_runtime::ErrorCode::ModelNotStoreOutput => 13,
        nixfied_runtime::ErrorCode::ModelInvalid => 14,
        nixfied_runtime::ErrorCode::PlatformUnsupported => 15,
        nixfied_runtime::ErrorCode::ClosureMissing => 16,
        nixfied_runtime::ErrorCode::SourceMismatch => 17,
        nixfied_runtime::ErrorCode::ModelAdmission => 18,
        nixfied_runtime::ErrorCode::RegistryCorrupt => 19,
        nixfied_runtime::ErrorCode::StateUnwritable => 20,
        nixfied_runtime::ErrorCode::StateUnowned => 21,
        nixfied_runtime::ErrorCode::CleanupRefused => 22,
        nixfied_runtime::ErrorCode::PortConflict => 23,
        nixfied_runtime::ErrorCode::PortUnverifiable => 24,
        nixfied_runtime::ErrorCode::ProcEscape => 25,
        nixfied_runtime::ErrorCode::ReadinessTimeout => 26,
        nixfied_runtime::ErrorCode::Canceled => 27,
        nixfied_runtime::ErrorCode::LeaseStale => 28,
        nixfied_runtime::ErrorCode::LeaseConflict => 29,
        nixfied_runtime::ErrorCode::TaskFailed => 30,
        nixfied_runtime::ErrorCode::LifecycleFailed => 31,
        nixfied_runtime::ErrorCode::DependencyUnavailable => 32,
        nixfied_runtime::ErrorCode::SecretUnavailable => 33,
        nixfied_runtime::ErrorCode::SecretLeakBlocked => 34,
    }
}
