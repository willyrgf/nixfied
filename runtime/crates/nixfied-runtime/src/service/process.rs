use std::collections::BTreeMap;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nixfied_model::{LifecycleOpClass, LifecycleOpSpec, Model, PortPolicy};
use serde::Serialize;

use crate::admission::Admission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::Registry;
use crate::service::identity::{service_address_hash, service_instance_id};
use crate::service::ownership::{ExpectedEndpointOwner, verify_endpoint_ownership};
use crate::service::readiness::wait_for_readiness_probe;
use crate::service::registry::{
    ProcessRecord, RunRecord, ServiceRecord, ensure_service_start_allowed,
    mark_endpoint_owner_verified, mark_process_escape, mark_service_failed,
    mark_service_probe_ready, mark_service_stopped, record_service_start,
};
use crate::state::HostPlacement;

const FOREGROUND_GRACE: Duration = Duration::from_millis(100);
const MONITOR_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedEndpoint {
    pub endpoint_id: String,
    pub host: String,
    pub port: u16,
}

pub struct StartedService {
    child: Child,
    monitor: ProcessMonitor,
    pub run_id: String,
    pub service_name: String,
    pub service_instance_id: String,
    pub process_key: String,
    pub pid: u32,
    pub pgid: i32,
    pub platform_start_identity: Option<String>,
    pub selected_endpoint: SelectedEndpoint,
    pub computed_model_hash: String,
}

impl StartedService {
    pub fn wait_for_probe_ready(
        &mut self,
        model: &Model,
        registry: &mut Registry,
    ) -> RuntimeResult<()> {
        let service = model.services.get(&self.service_name).ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("service {} is missing", self.service_name),
            )
        })?;
        let endpoint = service
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == self.selected_endpoint.endpoint_id)
            .ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::ModelAdmission,
                    format!("endpoint {} is missing", self.selected_endpoint.endpoint_id),
                )
            })?;
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            return Err(error);
        }
        self.ensure_alive_or_record_escape(registry)?;
        if let Err(error) = wait_for_readiness_probe(service, endpoint, self.selected_endpoint.port)
        {
            if let Some(error) = self.escape_error(registry) {
                self.cleanup_after_escape();
                return Err(error);
            }
            self.ensure_alive_or_record_escape(registry)?;
            self.cleanup_after_readiness_failure(registry, &error);
            return Err(error);
        }
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            return Err(error);
        }
        self.ensure_alive_or_record_escape(registry)?;
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            return Err(error);
        }
        let ownership = match verify_endpoint_ownership(
            endpoint,
            self.selected_endpoint.port,
            &ExpectedEndpointOwner {
                pid: self.pid,
                pgid: self.pgid,
                process_key: &self.process_key,
                platform_start_identity: self.platform_start_identity.as_deref(),
            },
        ) {
            Ok(ownership) => ownership,
            Err(error) => {
                if let Some(error) = self.escape_error(registry) {
                    self.cleanup_after_escape();
                    return Err(error);
                }
                self.ensure_alive_or_record_escape(registry)?;
                self.cleanup_after_readiness_failure(registry, &error);
                return Err(error);
            }
        };
        mark_endpoint_owner_verified(
            registry,
            &endpoint_key(
                &self.service_instance_id,
                &self.selected_endpoint.endpoint_id,
            ),
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
            &serde_json::to_string(&ownership)
                .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?,
        )?;
        mark_service_probe_ready(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
        )
    }

    pub fn stop(mut self, registry: &mut Registry, timeout_ms: u64) -> RuntimeResult<()> {
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            return Err(error);
        }
        match self.child.try_wait().map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect foreground service child: {error}"),
            )
        })? {
            Some(status) => {
                let message = format!("foreground service exited before stop: {status}");
                let error = self.record_escape(registry, message, Vec::new());
                self.cleanup_after_escape();
                return Err(error);
            }
            None => {}
        }
        signal_process_group(self.pgid, libc::SIGTERM)?;
        if !wait_for_child_exit(&mut self.child, timeout_ms)? {
            signal_process_group(self.pgid, libc::SIGKILL)?;
            if !wait_for_child_exit(&mut self.child, 1000)? {
                return Err(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    format!("failed to stop foreground service process {}", self.pid),
                ));
            }
        }
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            return Err(error);
        }
        self.monitor.stop();
        mark_service_stopped(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
        )
    }

    fn escape_error(&mut self, registry: &mut Registry) -> Option<RuntimeError> {
        let escaped = self.monitor.escaped_descendants();
        if escaped.is_empty() {
            return None;
        }
        let escaped_pids = escaped
            .iter()
            .map(|process| process.pid)
            .collect::<Vec<_>>();
        let message = format!(
            "service process {} has escaped descendants outside pgid {}: {:?}",
            self.pid, self.pgid, escaped_pids
        );
        Some(self.record_escape(registry, message, escaped_pids))
    }

    fn record_escape(
        &mut self,
        registry: &mut Registry,
        message: String,
        escaped: Vec<u32>,
    ) -> RuntimeError {
        let payload = serde_json::json!({
            "pid": self.pid,
            "pgid": self.pgid,
            "escapedDescendants": escaped,
            "reason": message,
        })
        .to_string();
        let _ = mark_process_escape(
            registry,
            &self.process_key,
            &self.run_id,
            &self.service_instance_id,
            &self.computed_model_hash,
            &payload,
        );
        RuntimeError::new(ErrorCode::ProcEscape, message)
    }

    fn ensure_alive_or_record_escape(&mut self, registry: &mut Registry) -> RuntimeResult<()> {
        match ensure_foreground_child_alive(self) {
            Ok(()) => Ok(()),
            Err(error) => {
                let recorded = self.record_escape(registry, error.message, Vec::new());
                self.cleanup_after_escape();
                Err(recorded)
            }
        }
    }

    fn cleanup_after_escape(&mut self) {
        let mut descendants = self
            .monitor
            .known_descendants()
            .into_iter()
            .map(|process| (process.pid, process))
            .collect::<BTreeMap<_, _>>();
        if let Ok(current) = descendant_pids(self.pid) {
            for pid in current {
                descendants
                    .entry(pid)
                    .or_insert_with(|| monitored_process(pid));
            }
        }
        best_effort_kill_processes(&descendants.into_values().collect::<Vec<_>>());
        let _ = terminate_process_group(self.pgid, 1000);
        let _ = self.child.wait();
        self.monitor.stop();
    }

    fn cleanup_after_readiness_failure(&mut self, registry: &mut Registry, error: &RuntimeError) {
        let _ = signal_process_group(self.pgid, libc::SIGTERM);
        if wait_for_child_exit(&mut self.child, 1000).ok() != Some(true) {
            let _ = signal_process_group(self.pgid, libc::SIGKILL);
            let _ = wait_for_child_exit(&mut self.child, 1000);
        }
        self.monitor.stop();
        let payload = serde_json::json!({
            "pid": self.pid,
            "pgid": self.pgid,
            "errorCode": error.code,
            "message": error.message.as_str(),
        })
        .to_string();
        let _ = mark_service_failed(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
            &payload,
        );
    }
}

impl Drop for StartedService {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = signal_process_group(self.pgid, libc::SIGTERM);
            if wait_for_child_exit(&mut self.child, 100).ok() != Some(true) {
                let _ = signal_process_group(self.pgid, libc::SIGKILL);
                let _ = wait_for_child_exit(&mut self.child, 1000);
            }
        }
        self.monitor.stop();
    }
}

pub fn start_synthetic_service(
    model: &Model,
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    selected_port: u16,
) -> RuntimeResult<StartedService> {
    let run_id = run_id.into();
    let service_name = "synthetic";
    let service = model.services.get(service_name).ok_or_else(|| {
        RuntimeError::new(ErrorCode::ModelAdmission, "M0 synthetic service is missing")
    })?;
    if !service.foreground {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            "M0 services must run in the foreground",
        ));
    }
    let start_op = lifecycle_op(service, LifecycleOpClass::Start)?;
    let exec_id = start_op.exec_id.as_deref().ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            "M0 service start operation must bind an exec",
        )
    })?;
    let exec = model.execs.get(exec_id).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("service start exec {exec_id} is missing"),
        )
    })?;
    let selected_endpoint = select_endpoint(service, selected_port)?;
    let address_hash = service_address_hash(model, service_name);
    let service_instance_id = service_instance_id(&address_hash, &service.identity);
    ensure_service_start_allowed(registry, &run_id, &service_instance_id)?;
    let args = operation_args(&exec.args, &start_op.exec_args, selected_port);
    let stdout_path = placement.logs_dir.join("service.synthetic.stdout.log");
    let stderr_path = placement.logs_dir.join("service.synthetic.stderr.log");
    let command_json = serde_json::to_string(&CommandRecord {
        executable: exec.executable.as_str(),
        args: &args,
        cwd: exec.cwd.as_str(),
        stdout_path: stdout_path.as_path(),
        stderr_path: stderr_path.as_path(),
    })
    .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    let mut command = Command::new(&exec.executable);
    command
        .args(&args)
        .current_dir(&exec.cwd)
        .envs(&exec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::from(create_log_file(&stdout_path)?))
        .stderr(Stdio::from(create_log_file(&stderr_path)?));
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut child = command.spawn().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to spawn synthetic service: {error}"),
        )
    })?;
    let pid = child.id();
    let pgid = match get_process_group(pid) {
        Ok(pgid) => pgid,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let platform_start = platform_start_identity(pid);
    let start_identity = process_start_identity(pid, pgid, platform_start.as_deref());
    let process_key = format!("process-{run_id}-{pid}-{pgid}");
    let endpoint_json = serde_json::to_string(&selected_endpoint)
        .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    if let Err(error) = record_service_start(
        registry,
        &RunRecord {
            run_id: &run_id,
            model,
            admission,
            placement,
        },
        &ServiceRecord {
            service_instance_id: &service_instance_id,
            service_name,
            service_address_hash: &address_hash,
            service,
            endpoint_json: &endpoint_json,
            state_root: &placement.state_root,
            endpoint_key: &endpoint_key(&service_instance_id, &selected_endpoint.endpoint_id),
            endpoint_address: &selected_endpoint.host,
            endpoint_port: selected_endpoint.port,
        },
        &ProcessRecord {
            process_key: &process_key,
            pid,
            pgid,
            start_identity: &start_identity,
            command_json: &command_json,
            run_id: &run_id,
            service_instance_id: &service_instance_id,
        },
    ) {
        let _ = terminate_process_group(pgid, 1000);
        let _ = child.wait();
        return Err(error);
    }
    let mut started = StartedService {
        child,
        monitor: spawn_process_monitor(pid, pgid),
        run_id,
        service_name: service_name.to_string(),
        service_instance_id,
        process_key,
        pid,
        pgid,
        platform_start_identity: platform_start,
        selected_endpoint,
        computed_model_hash: admission.computed_model_hash.clone(),
    };
    if let Err(error) = ensure_foreground_child_alive(&mut started) {
        let payload = serde_json::json!({
            "pid": started.pid,
            "pgid": started.pgid,
            "reason": error.message,
        })
        .to_string();
        let _ = mark_process_escape(
            registry,
            &started.process_key,
            &started.run_id,
            &started.service_instance_id,
            &started.computed_model_hash,
            &payload,
        );
        started.cleanup_after_escape();
        return Err(error);
    }
    Ok(started)
}

fn lifecycle_op(
    service: &nixfied_model::ServiceSpec,
    class: LifecycleOpClass,
) -> RuntimeResult<&LifecycleOpSpec> {
    service
        .lifecycle
        .iter()
        .find(|operation| operation.class == class)
        .ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("service lifecycle operation {class:?} is missing"),
            )
        })
}

fn select_endpoint(
    service: &nixfied_model::ServiceSpec,
    selected_port: u16,
) -> RuntimeResult<SelectedEndpoint> {
    let endpoint = service.endpoints.first().ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            "M0 synthetic service requires one endpoint",
        )
    })?;
    match &endpoint.port {
        PortPolicy::Fixed { port } if *port != selected_port => {
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("selected port {selected_port} does not match fixed port {port}"),
            ));
        }
        PortPolicy::CandidateWindow { start, end }
            if selected_port < *start || selected_port > *end =>
        {
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("selected port {selected_port} is outside candidate window {start}-{end}"),
            ));
        }
        _ => {}
    }
    Ok(SelectedEndpoint {
        endpoint_id: endpoint.endpoint_id.clone(),
        host: endpoint.host.clone(),
        port: selected_port,
    })
}

fn operation_args(base_args: &[String], op_args: &[String], selected_port: u16) -> Vec<String> {
    base_args
        .iter()
        .chain(op_args.iter())
        .map(|arg| arg.replace("${port}", &selected_port.to_string()))
        .collect()
}

fn endpoint_key(service_instance_id: &str, endpoint_id: &str) -> String {
    format!("{service_instance_id}:{endpoint_id}")
}

fn create_log_file(path: &Path) -> RuntimeResult<File> {
    File::create(path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("failed to create log file {}: {error}", path.display()),
        )
    })
}

fn get_process_group(pid: u32) -> RuntimeResult<i32> {
    process_group(pid)?.ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("process {pid} no longer exists"),
        )
    })
}

pub(crate) fn process_group(pid: u32) -> RuntimeResult<Option<i32>> {
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    if pgid < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to inspect process group for pid {pid}: {}", error),
        ))
    } else {
        Ok(Some(pgid))
    }
}

pub(crate) fn process_is_live_with_identity(
    pid: u32,
    pgid: i32,
    platform_start: Option<&str>,
) -> RuntimeResult<bool> {
    let Some(current_pgid) = process_group(pid)? else {
        return Ok(false);
    };
    if current_pgid != pgid {
        return Ok(false);
    }
    if let Some(expected) = platform_start {
        if platform_start_identity(pid).as_deref() != Some(expected) {
            return Ok(false);
        }
    }
    Ok(!process_is_zombie(pid))
}

pub(crate) fn process_group_has_live_member(pgid: i32) -> RuntimeResult<bool> {
    process_group_has_live_member_impl(pgid)
}

fn ensure_foreground_child_alive(service: &mut StartedService) -> RuntimeResult<()> {
    thread::sleep(FOREGROUND_GRACE);
    match service.child.try_wait().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to inspect service child: {error}"),
        )
    })? {
        None => Ok(()),
        Some(status) => Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("foreground service exited before handoff: {status}"),
        )),
    }?;
    let current_pgid = get_process_group(service.pid)?;
    if current_pgid != service.pgid {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "foreground service process {} moved from pgid {} to pgid {current_pgid}",
                service.pid, service.pgid
            ),
        ));
    }
    let escaped = escaped_descendants(service.pid, service.pgid)?;
    if !escaped.is_empty() {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "service process {} has descendants outside pgid {}: {:?}",
                service.pid, service.pgid, escaped
            ),
        ));
    }
    Ok(())
}

fn terminate_process_group(pgid: i32, timeout_ms: u64) -> RuntimeResult<()> {
    signal_process_group(pgid, libc::SIGTERM)?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if process_group_is_gone(pgid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    signal_process_group(pgid, libc::SIGKILL)
}

pub(crate) fn wait_for_child_exit(child: &mut Child, timeout_ms: u64) -> RuntimeResult<bool> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if child
            .try_wait()
            .map_err(|error| RuntimeError::new(ErrorCode::ProcEscape, error.to_string()))?
            .is_some()
        {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(25));
    }
    Ok(false)
}

pub(crate) fn signal_process_group(pgid: i32, signal: i32) -> RuntimeResult<()> {
    let result = unsafe { libc::kill(-pgid, signal) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "failed to signal process group {pgid}: {}",
                std::io::Error::last_os_error()
            ),
        ))
    }
}

fn process_group_is_gone(pgid: i32) -> bool {
    let result = unsafe { libc::kill(-pgid, 0) };
    result != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MonitoredProcess {
    pid: u32,
    platform_start: Option<String>,
}

#[derive(Default)]
struct ProcessMonitorState {
    known_descendants: BTreeMap<u32, MonitoredProcess>,
    escaped_descendants: BTreeMap<u32, MonitoredProcess>,
}

struct ProcessMonitor {
    state: Arc<Mutex<ProcessMonitorState>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProcessMonitor {
    fn escaped_descendants(&self) -> Vec<MonitoredProcess> {
        self.state
            .lock()
            .map(|state| state.escaped_descendants.values().cloned().collect())
            .unwrap_or_default()
    }

    fn known_descendants(&self) -> Vec<MonitoredProcess> {
        self.state
            .lock()
            .map(|state| state.known_descendants.values().cloned().collect())
            .unwrap_or_default()
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ProcessMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn_process_monitor(pid: u32, expected_pgid: i32) -> ProcessMonitor {
    let state = Arc::new(Mutex::new(ProcessMonitorState::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let thread_state = Arc::clone(&state);
    let thread_stop = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !thread_stop.load(Ordering::SeqCst) {
            collect_process_tree(pid, expected_pgid, &thread_state);
            thread::sleep(MONITOR_INTERVAL);
        }
        collect_process_tree(pid, expected_pgid, &thread_state);
    });
    ProcessMonitor {
        state,
        stop,
        handle: Some(handle),
    }
}

fn collect_process_tree(pid: u32, expected_pgid: i32, state: &Arc<Mutex<ProcessMonitorState>>) {
    let Ok(descendants) = descendant_pids(pid) else {
        return;
    };
    let mut escaped = Vec::new();
    for descendant in &descendants {
        if let Ok(Some(pgid)) = process_group(*descendant) {
            if pgid != expected_pgid {
                escaped.push(monitored_process(*descendant));
            }
        }
    }
    if let Ok(mut state) = state.lock() {
        for descendant in descendants {
            state
                .known_descendants
                .entry(descendant)
                .or_insert_with(|| monitored_process(descendant));
        }
        for process in escaped {
            state.escaped_descendants.insert(process.pid, process);
        }
    }
}

fn monitored_process(pid: u32) -> MonitoredProcess {
    MonitoredProcess {
        pid,
        platform_start: platform_start_identity(pid),
    }
}

fn best_effort_kill_processes(processes: &[MonitoredProcess]) {
    for process in processes {
        if same_process_identity(process) {
            unsafe {
                libc::kill(process.pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }
    thread::sleep(Duration::from_millis(25));
    for process in processes {
        if same_process_identity(process) {
            unsafe {
                libc::kill(process.pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }
}

fn same_process_identity(process: &MonitoredProcess) -> bool {
    let Some(expected) = process.platform_start.as_deref() else {
        return false;
    };
    platform_start_identity(process.pid).as_deref() == Some(expected)
}

fn escaped_descendants(pid: u32, expected_pgid: i32) -> RuntimeResult<Vec<u32>> {
    let mut escaped = Vec::new();
    for descendant in descendant_pids(pid)? {
        if let Some(pgid) = process_group(descendant)? {
            if pgid != expected_pgid {
                escaped.push(descendant);
            }
        }
    }
    Ok(escaped)
}

#[cfg(target_os = "linux")]
fn descendant_pids(pid: u32) -> RuntimeResult<Vec<u32>> {
    let mut descendants = Vec::new();
    let mut queue = vec![pid];
    while let Some(parent) = queue.pop() {
        for child in direct_child_pids_linux(parent)? {
            queue.push(child);
            descendants.push(child);
        }
    }
    Ok(descendants)
}

#[cfg(target_os = "linux")]
fn direct_child_pids_linux(parent: u32) -> RuntimeResult<Vec<u32>> {
    let mut children = Vec::new();
    let entries = std::fs::read_dir("/proc").map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to inspect /proc for process descendants: {error}"),
        )
    })?;
    for entry in entries {
        let entry =
            entry.map_err(|error| RuntimeError::new(ErrorCode::ProcEscape, error.to_string()))?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        let status = std::fs::read_to_string(entry.path().join("status"));
        let Ok(status) = status else {
            continue;
        };
        let Some(ppid) = status
            .lines()
            .find_map(|line| line.strip_prefix("PPid:"))
            .and_then(|value| value.trim().parse::<u32>().ok())
        else {
            continue;
        };
        if ppid == parent {
            children.push(pid);
        }
    }
    Ok(children)
}

#[cfg(target_os = "macos")]
fn descendant_pids(pid: u32) -> RuntimeResult<Vec<u32>> {
    let mut descendants = Vec::new();
    let mut queue = vec![pid];
    while let Some(parent) = queue.pop() {
        for child in direct_child_pids_macos(parent)? {
            queue.push(child);
            descendants.push(child);
        }
    }
    Ok(descendants)
}

#[cfg(target_os = "macos")]
fn direct_child_pids_macos(parent: u32) -> RuntimeResult<Vec<u32>> {
    let capacity = 8192usize;
    let mut buffer = vec![0 as libc::pid_t; capacity];
    let count = unsafe {
        libc::proc_listallpids(
            buffer.as_mut_ptr().cast(),
            (capacity * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
        )
    };
    if count < 0 {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "failed to list pids while inspecting descendants for pid {parent}: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let mut children = Vec::new();
    for pid in buffer.into_iter().take(count as usize) {
        let Ok(pid) = u32::try_from(pid) else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        let Some(info) = process_bsd_info(pid) else {
            continue;
        };
        if info.pbi_ppid == parent {
            children.push(pid);
        }
    }
    Ok(children)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn descendant_pids(_pid: u32) -> RuntimeResult<Vec<u32>> {
    Ok(Vec::new())
}

fn process_start_identity(pid: u32, pgid: i32, platform_start: Option<&str>) -> String {
    let observed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    serde_json::json!({
        "pid": pid,
        "pgid": pgid,
        "platformStart": platform_start,
        "observedAtNanos": observed_at,
    })
    .to_string()
}

#[cfg(target_os = "linux")]
pub(crate) fn platform_start_identity(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields_after_comm = stat.rsplit_once(") ")?.1;
    let start_time_ticks = fields_after_comm.split_whitespace().nth(19)?;
    Some(format!("linux-start-ticks:{start_time_ticks}"))
}

#[cfg(target_os = "linux")]
fn process_is_zombie(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    stat.rsplit_once(") ")
        .and_then(|(_, fields)| fields.split_whitespace().next())
        == Some("Z")
}

#[cfg(target_os = "linux")]
fn process_group_has_live_member_impl(pgid: i32) -> RuntimeResult<bool> {
    let entries = std::fs::read_dir("/proc").map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to inspect /proc while checking process group {pgid}: {error}"),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect /proc while checking process group {pgid}: {error}"),
            )
        })?;
        let Some(pid) = entry.file_name().to_string_lossy().parse::<u32>().ok() else {
            continue;
        };
        if process_group(pid)? == Some(pgid) && !process_is_zombie(pid) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_start_identity(pid: u32) -> Option<String> {
    let info = process_bsd_info(pid)?;
    Some(format!(
        "macos-start-time:{}:{}",
        info.pbi_start_tvsec, info.pbi_start_tvusec
    ))
}

#[cfg(target_os = "macos")]
fn process_is_zombie(pid: u32) -> bool {
    process_bsd_info(pid)
        .map(|info| info.pbi_status == libc::SZOMB)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn process_group_has_live_member_impl(pgid: i32) -> RuntimeResult<bool> {
    let capacity = 8192usize;
    let mut buffer = vec![0 as libc::pid_t; capacity];
    let count = unsafe {
        libc::proc_listallpids(
            buffer.as_mut_ptr().cast(),
            (capacity * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
        )
    };
    if count < 0 {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "failed to list pids while checking process group {pgid}: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    for pid in buffer.into_iter().take(count as usize) {
        let Ok(pid) = u32::try_from(pid) else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        if process_group(pid)? == Some(pgid) && !process_is_zombie(pid) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "macos")]
fn process_bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let result = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if result != size {
        return None;
    }
    Some(unsafe { info.assume_init() })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn platform_start_identity(_pid: u32) -> Option<String> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_is_zombie(_pid: u32) -> bool {
    false
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_group_has_live_member_impl(_pgid: i32) -> RuntimeResult<bool> {
    Ok(false)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandRecord<'a> {
    executable: &'a str,
    args: &'a [String],
    cwd: &'a str,
    stdout_path: &'a Path,
    stderr_path: &'a Path,
}
