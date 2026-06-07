use std::path::PathBuf;

use nixfied_runtime::cancellation::{CancellationToken, ProcessSignalGuard};
use nixfied_runtime::registry::{Registry, RegistryIdentity, RunLeaseHeartbeat};
use nixfied_runtime::service::{
    run_dependent_task_cancellable, run_synthetic_service_clean_for_slot,
    start_synthetic_service_for_slot,
};
use nixfied_runtime::slot::{first_candidate_port, select_slot};
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
struct RunOutput {
    run_id: String,
    model_path: PathBuf,
    computed_model_hash: String,
    service_instance_id: String,
    process_key: String,
    selected_endpoint: nixfied_runtime::service::SelectedEndpoint,
    task: nixfied_runtime::service::task::TaskRun,
    summary_path: PathBuf,
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
    let selected_port = first_candidate_port(&selected_slot.placement.candidate_ports)?;
    let mut service = start_synthetic_service_for_slot(
        model,
        admission,
        &placement,
        &mut registry,
        run_id.clone(),
        &selected_slot,
        selected_port,
    )?;
    let lease_heartbeat = RunLeaseHeartbeat::start(
        placement.registry_path().to_path_buf(),
        registry.identity().clone(),
        service.run_id.clone(),
        service.owner_token.clone(),
    );
    if let Err(error) = service.wait_for_probe_ready_cancellable(model, &mut registry, cancellation)
    {
        if error.code == nixfied_runtime::ErrorCode::Canceled {
            lease_heartbeat.stop()?;
            service.cancel(
                &mut registry,
                options.timeout_ms,
                "run canceled during readiness",
            )?;
        } else {
            lease_heartbeat.stop()?;
            let _ = service.stop(&mut registry, options.timeout_ms);
        }
        return Err(error);
    }
    if let Err(error) = service.check_health_cancellable(model, &mut registry, cancellation) {
        if error.code == nixfied_runtime::ErrorCode::Canceled {
            lease_heartbeat.stop()?;
            service.cancel(
                &mut registry,
                options.timeout_ms,
                "run canceled during health check",
            )?;
        } else {
            lease_heartbeat.stop()?;
            let _ = service.stop(&mut registry, options.timeout_ms);
        }
        return Err(error);
    }
    if let Err(error) = cancellation.check() {
        lease_heartbeat.stop()?;
        service.cancel(
            &mut registry,
            options.timeout_ms,
            "run canceled after readiness",
        )?;
        return Err(error);
    }
    let task = match run_dependent_task_cancellable(
        model,
        &placement,
        &mut registry,
        &service,
        "smoke",
        cancellation,
    ) {
        Ok(task) => task,
        Err(error) => {
            if error.code == nixfied_runtime::ErrorCode::Canceled {
                lease_heartbeat.stop()?;
                service.cancel(
                    &mut registry,
                    options.timeout_ms,
                    "run canceled during task",
                )?;
            } else {
                lease_heartbeat.stop()?;
                let _ = service.stop(&mut registry, options.timeout_ms);
            }
            return Err(error);
        }
    };
    let output = RunOutput {
        run_id,
        model_path: admission.model_path.clone(),
        computed_model_hash: admission.computed_model_hash.clone(),
        service_instance_id: service.service_instance_id.clone(),
        process_key: service.process_key.clone(),
        selected_endpoint: service.selected_endpoint.clone(),
        summary_path: task.summary_path.clone(),
        task,
    };
    if cancellation.is_canceled() {
        lease_heartbeat.stop()?;
        service.cancel(
            &mut registry,
            options.timeout_ms,
            "run canceled during shutdown",
        )?;
        return Err(nixfied_runtime::cancellation::canceled_error());
    }
    lease_heartbeat.stop()?;
    service.stop_cancellable(&mut registry, options.timeout_ms, cancellation)?;
    Ok(output)
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
        ControlCommand::Clean => {
            print_json(&run_synthetic_service_clean_for_slot(
                model,
                admission,
                &placement,
                &mut registry,
                &selected_slot,
            )?)
        }
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
