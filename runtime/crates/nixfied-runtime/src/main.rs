use std::path::PathBuf;

use nixfied_runtime::cancellation::{CancellationToken, ProcessSignalGuard};
use nixfied_runtime::registry::{Registry, RegistryIdentity, RunLeaseHeartbeat};
use nixfied_runtime::service::task::TaskRun;
use nixfied_runtime::service::{
    SelectedEndpoint, StartedService, run_dependent_task_cancellable, run_slot_clean,
    start_service_for_slot,
};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{
    StateIdentity, derive_host_placement_for_slot, materialize_run_roots, state_base_from_env,
    write_slot_marker,
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
    selected_endpoint: SelectedEndpoint,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunOutput {
    run_id: String,
    model_path: PathBuf,
    computed_model_hash: String,
    services: Vec<ServiceRunOutput>,
    tasks: Vec<TaskRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<TaskRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary_path: Option<PathBuf>,
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
            format!("unsupported M0 runtime command: {command}"),
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
    let model_path = admission.model_path.clone();
    let computed_model_hash = admission.computed_model_hash.clone();
    let output = run_m0_admitted(&loaded.model, &admission, &options, run_id, &cancellation)
        .map_err(|error| error.with_model_if_missing(model_path, computed_model_hash))?;
    print_json(&output)
}

fn run_m0_admitted(
    model: &nixfied_model::Model,
    admission: &Admission,
    options: &RunOptions,
    run_id: String,
    cancellation: &CancellationToken,
) -> Result<RunOutput, RuntimeError> {
    cancellation.check()?;
    let selected_slot = select_slot(model, options.selection.slot)?;
    let placement =
        derive_host_placement_for_slot(model, &selected_slot, &run_id, &options.state_base)?;
    materialize_run_roots(&placement)?;
    let identity = StateIdentity::from_selected_slot(model, admission, &selected_slot);
    write_slot_marker(&placement, &identity)?;
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
    let _ = nixfied_runtime::control::reconcile_registry(&mut registry)?;

    let environment = model
        .environments
        .get(selected_slot.environment)
        .ok_or_else(|| {
            RuntimeError::new(
                nixfied_runtime::ErrorCode::ModelAdmission,
                format!("environment {} is missing", selected_slot.environment),
            )
        })?;
    let window = &selected_slot.placement.candidate_ports;

    // Start every service in the environment, each on a distinct port within the
    // slot's candidate window, waiting readiness then health before the next.
    let mut started: Vec<StartedService> = Vec::new();
    let mut lease: Option<RunLeaseHeartbeat> = None;
    for (index, service_name) in environment.services.iter().enumerate() {
        let offset = u16::try_from(index).ok().filter(|offset| {
            window
                .start
                .checked_add(*offset)
                .is_some_and(|port| port <= window.end)
        });
        let Some(offset) = offset else {
            let error = RuntimeError::new(
                nixfied_runtime::ErrorCode::PortConflict,
                format!(
                    "slot {} candidate window {}-{} cannot host {} services",
                    selected_slot.slot,
                    window.start,
                    window.end,
                    environment.services.len()
                ),
            );
            teardown(&mut started, &mut registry, options.timeout_ms, false);
            stop_lease(lease)?;
            return Err(error);
        };
        let selected_port = window.start + offset;

        let started_service = match start_service_for_slot(
            model,
            admission,
            &placement,
            &mut registry,
            run_id.clone(),
            &selected_slot,
            service_name,
            selected_port,
        ) {
            Ok(service) => service,
            Err(error) => {
                teardown(
                    &mut started,
                    &mut registry,
                    options.timeout_ms,
                    cancellation.is_canceled(),
                );
                stop_lease(lease)?;
                return Err(error);
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
        if let Err(error) =
            service.wait_for_probe_ready_cancellable(model, &mut registry, cancellation)
        {
            teardown(
                &mut started,
                &mut registry,
                options.timeout_ms,
                error.code == nixfied_runtime::ErrorCode::Canceled,
            );
            stop_lease(lease)?;
            return Err(error);
        }
        let service = started.last_mut().expect("just pushed a service");
        if let Err(error) = service.check_health_cancellable(model, &mut registry, cancellation) {
            teardown(
                &mut started,
                &mut registry,
                options.timeout_ms,
                error.code == nixfied_runtime::ErrorCode::Canceled,
            );
            stop_lease(lease)?;
            return Err(error);
        }
    }

    if let Err(error) = cancellation.check() {
        teardown(&mut started, &mut registry, options.timeout_ms, true);
        stop_lease(lease)?;
        return Err(error);
    }

    // Run every task, gating each on the readiness of its declared service
    // dependency (the first dependency provides ${port}/${host} substitution).
    let mut task_runs: Vec<TaskRun> = Vec::new();
    for task_id in &environment.tasks {
        let task = model.tasks.get(task_id).ok_or_else(|| {
            RuntimeError::new(
                nixfied_runtime::ErrorCode::ModelAdmission,
                format!("task {task_id} is missing"),
            )
        })?;
        let dependency = match task.depends_on_services_ready.first() {
            Some(name) => started.iter().find(|service| &service.service_name == name),
            None => started.first(),
        };
        let Some(dependency) = dependency else {
            let error = RuntimeError::new(
                nixfied_runtime::ErrorCode::ModelAdmission,
                format!("task {task_id} has no started service to depend on"),
            );
            teardown(&mut started, &mut registry, options.timeout_ms, false);
            stop_lease(lease)?;
            return Err(error);
        };
        match run_dependent_task_cancellable(
            model,
            &placement,
            &mut registry,
            dependency,
            task_id,
            cancellation,
        ) {
            Ok(task_run) => task_runs.push(task_run),
            Err(error) => {
                teardown(
                    &mut started,
                    &mut registry,
                    options.timeout_ms,
                    error.code == nixfied_runtime::ErrorCode::Canceled,
                );
                stop_lease(lease)?;
                return Err(error);
            }
        }
    }

    let services_output = started
        .iter()
        .map(|service| ServiceRunOutput {
            service_id: service.service_name.clone(),
            service_instance_id: service.service_instance_id.clone(),
            process_key: service.process_key.clone(),
            selected_endpoint: service.selected_endpoint.clone(),
        })
        .collect::<Vec<_>>();
    let primary_task = task_runs.last().cloned();
    let output = RunOutput {
        run_id,
        model_path: admission.model_path.clone(),
        computed_model_hash: admission.computed_model_hash.clone(),
        services: services_output,
        summary_path: primary_task.as_ref().map(|task| task.summary_path.clone()),
        task: primary_task,
        tasks: task_runs,
    };

    if cancellation.is_canceled() {
        teardown(&mut started, &mut registry, options.timeout_ms, true);
        stop_lease(lease)?;
        return Err(nixfied_runtime::cancellation::canceled_error());
    }
    // Stop services in reverse start order.
    while let Some(service) = started.pop() {
        service.stop_cancellable(&mut registry, options.timeout_ms, cancellation)?;
    }
    stop_lease(lease)?;
    Ok(output)
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
}

struct RunOptions {
    model_path: PathBuf,
    allow_non_store: bool,
    state_base: PathBuf,
    timeout_ms: u64,
    selection: RuntimeSelection,
}

fn run_control(command: ControlCommand, args: &[String]) -> Result<(), RuntimeError> {
    let options = parse_control_options(command, args)?;
    let (loaded, admission) =
        load_admitted_model(options.model_path.clone(), options.allow_non_store)?;
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
        )?),
    }
}

fn parse_run_options(args: &[String]) -> Result<RunOptions, RuntimeError> {
    let mut model_path = None;
    let mut allow_non_store = false;
    let mut state_base = None;
    let mut timeout_ms = 5000;
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
            "--state-base" => {
                index += 1;
                state_base = args.get(index).map(PathBuf::from);
            }
            "--slot" => {
                index += 1;
                slot = Some(parse_slot_arg(args.get(index), "--slot")?);
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
    let policy = if allow_non_store {
        StoreOriginPolicy::AllowNonStoreForTests
    } else {
        StoreOriginPolicy::RequireStore
    };
    let context = AdmissionContext::current(policy);
    let raw_model = read_raw_model(&model_path)?;
    nixfied_runtime::admission::origin::check_raw_store_origin(&raw_model, &context)?;
    let loaded = parse_loaded_model(raw_model)?;
    let admission = Admission::check(&loaded, &context)?;
    Ok((loaded, admission))
}

fn print_json(value: &impl Serialize) -> Result<(), RuntimeError> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| RuntimeError::new(
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
    }
}
