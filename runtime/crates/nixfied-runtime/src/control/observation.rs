//! Native session projections and run-pinned observation. Registry facts and
//! leader samples remain separate until final successful construction here.
use serde::Serialize;
use std::time::Duration;

use crate::admission::ControlAdmission;
use crate::cancellation::{CancellationToken, WaitBudget};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::error::{
    ObservationEstablishment, ObservationEvidence, ObservationSelection, ObservationStage,
    ObservationWait, RequestMismatchField, SessionUnavailableReason, request_mismatch,
    session_unavailable,
};
use crate::output::utf8_locator;
use crate::redaction::CaptureOutcome;
use crate::registry::observation::{
    CandidateSnapshot, CaptureState, History, PinnedRun, ProcessFacts, Scope, ServiceFacts,
    Snapshot,
};
use crate::registry::session::{
    ExecutionOutcome, OwnerIdentity, RecordedRunRequest, SessionProgress, valid_run_id,
};
use crate::registry::status::{DbStatus, Ownership, ProcessStatus};
use crate::service::{LeaderSample, sample_leader};
use crate::state::SlotLocation;

pub use crate::registry::observation::EndpointCoordinate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    project_id: String,
    environment: String,
    slot: u32,
    state_base: String,
    state_root: String,
    registry_dir: String,
    registry_path: String,
}

impl Context {
    pub(crate) fn from_location(location: &SlotLocation) -> RuntimeResult<Self> {
        Ok(Self {
            project_id: location.slot().project().to_owned(),
            environment: location.slot().environment().to_owned(),
            slot: location.slot().slot(),
            state_base: utf8_locator(location.state_base(), "stateBase")?.to_owned(),
            state_root: utf8_locator(&location.state_root(), "stateRoot")?.to_owned(),
            registry_dir: utf8_locator(&location.registry_dir(), "registryDir")?.to_owned(),
            registry_path: utf8_locator(&location.registry_path(), "registryPath")?.to_owned(),
        })
    }
}

pub fn context(admission: &ControlAdmission, location: &SlotLocation) -> RuntimeResult<Context> {
    Scope::new(admission, location)?;
    Context::from_location(location)
}

/// Checked selector syntax. Choosing it does not select a publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector(SelectorKind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum SelectorKind {
    Current,
    RunId(String),
}

impl Selector {
    pub fn current() -> Self {
        Self(SelectorKind::Current)
    }
    pub fn run_id(run: &str) -> RuntimeResult<Self> {
        if !valid_run_id(run) {
            return Err(RuntimeError::new(
                ErrorCode::ManifestAdmission,
                "run selector must contain 1–128 ASCII alphanumeric or hyphen bytes",
            ));
        }
        Ok(Self(SelectorKind::RunId(run.to_owned())))
    }
    pub(crate) fn exact_run(&self) -> Option<&str> {
        match &self.0 {
            SelectorKind::Current => None,
            SelectorKind::RunId(run) => Some(run),
        }
    }
}

/// Lexical root-step syntax is metadata; only the planner proves membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct RootStepPath(String);

impl RootStepPath {
    pub fn parse(path: &str) -> RuntimeResult<Self> {
        if !path
            .split('.')
            .all(nixfied_manifest::validation::is_step_safe_id)
        {
            return Err(RuntimeError::new(
                ErrorCode::TaskSelectionInvalid,
                "observation step must be a root leaf path",
            ));
        }
        Ok(Self(path.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

include!("../generated/observation.rs");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leader {
    pid: u32,
    start_identity: String,
    observation: LeaderObservation,
}

impl Leader {
    fn sample(pid: u32, start: &str, group: Option<i32>) -> RuntimeResult<Self> {
        let observation = match sample_leader(pid)? {
            LeaderSample::Absent | LeaderSample::Present { zombie: true, .. } => {
                LeaderObservation::Absent
            }
            LeaderSample::Present {
                start_identity,
                pgid,
                zombie: false,
            } => {
                if start_identity != start || group.is_some_and(|expected| expected != pgid) {
                    LeaderObservation::IdentityMismatch
                } else {
                    LeaderObservation::Live
                }
            }
        };
        Ok(Self {
            pid,
            start_identity: start.to_owned(),
            observation,
        })
    }
    fn owner(owner: &OwnerIdentity) -> RuntimeResult<Self> {
        Self::sample(owner.pid(), owner.start_identity(), None)
    }
    fn service(service: &ProcessFacts) -> RuntimeResult<Self> {
        Self::sample(service.pid, &service.start, Some(service.pgid))
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum Progress {
    OutcomePending,
    Finalizing {
        #[serde(serialize_with = "serialize_outcome")]
        outcome: ExecutionOutcome,
    },
    Finalized {
        #[serde(serialize_with = "serialize_outcome")]
        outcome: ExecutionOutcome,
    },
}

fn serialize_outcome<S: serde::Serializer>(
    outcome: &ExecutionOutcome,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(outcome.as_str())
}

impl From<SessionProgress> for Progress {
    fn from(progress: SessionProgress) -> Self {
        match progress {
            SessionProgress::Executing => Self::OutcomePending,
            SessionProgress::Finalizing(outcome) => Self::Finalizing { outcome },
            SessionProgress::Finalized(outcome) => Self::Finalized { outcome },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLeader {
    service_id: String,
    process_key: String,
    leader: Leader,
    endpoints: Vec<EndpointCoordinate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    run_id: String,
    manifest_path: String,
    request: RecordedRunRequest,
    progress: Progress,
    run_dir: String,
    logs_dir: String,
    owner: Leader,
    services: Vec<ServiceLeader>,
}

impl Session {
    fn sample(candidate: &CandidateSnapshot, pinned: &PinnedRun) -> RuntimeResult<Self> {
        let placement = pinned.scope().location().for_run(pinned.run_id())?;
        let owner = Leader::owner(&candidate.run.owner)?;
        let services = candidate
            .services
            .iter()
            .map(|service| {
                Ok(ServiceLeader {
                    service_id: service.service_id.clone(),
                    process_key: service.process.key.clone(),
                    leader: Leader::service(&service.process)?,
                    endpoints: service.endpoints.clone(),
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;
        Ok(Self {
            run_id: pinned.run_id().to_owned(),
            manifest_path: candidate.run.manifest_path.clone(),
            request: candidate.run.request.clone(),
            progress: candidate.run.progress.into(),
            run_dir: utf8_locator(&placement.run_dir(), "runDir")?.to_owned(),
            logs_dir: utf8_locator(&placement.logs_dir(), "logsDir")?.to_owned(),
            owner,
            services,
        })
    }
}

/// Only the successful observation path can construct these evidence records.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedRootTask {
    step_path: String,
    task_id: String,
    process_key: String,
    exit_code: i32,
    stdout_path: String,
    stderr_path: String,
}

/// A successful observation is minted by the native checking boundary.
/// Stored bytes cannot construct this proof:
///
/// ```compile_fail
/// use nixfied_runtime::control::observation::TaskObservation;
/// let fabricated: TaskObservation = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Serialize)]
pub struct TaskObservation {
    context: Context,
    session: Session,
    task: CompletedRootTask,
}

#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "kebab-case")]
pub enum ObserveOutput {
    Absent { context: Context },
    Session { context: Context, session: Session },
    Satisfied { observation: TaskObservation },
}

fn pause(budget: &WaitBudget) {
    std::thread::sleep(budget.remaining().min(Duration::from_millis(10)));
}

fn select(
    admission: &ControlAdmission,
    location: &SlotLocation,
    selector: &Selector,
    wait: &ObservationWait<'_>,
) -> RuntimeResult<(Snapshot, Option<crate::registry::observation::Publication>)> {
    let scope = Scope::new(admission, location)?;
    let mut history = History::default();
    loop {
        wait.checkpoint(
            ObservationStage::Selection,
            &ObservationSelection::unenrolled(),
        )?;
        if let Some(snapshot) = Snapshot::read(&scope, &mut history)? {
            let publication = match selector.exact_run() {
                None => snapshot.current()?,
                Some(run) => snapshot.publication(run).cloned(),
            };
            return Ok((snapshot, publication));
        }
        pause(wait.budget);
    }
}

pub fn inspect(
    admission: &ControlAdmission,
    location: &SlotLocation,
    selector: &Selector,
    budget: &WaitBudget,
    cancellation: &CancellationToken,
) -> RuntimeResult<ObserveOutput> {
    let establishment = ObservationEstablishment::not_requested();
    let wait = ObservationWait {
        budget,
        cancellation,
        establishment: &establishment,
        step: None,
    };
    let context = context(admission, location)?;
    let (snapshot, selected) = select(admission, location, selector, &wait)?;
    let Some(publication) = selected else {
        wait.checkpoint(
            ObservationStage::Selection,
            &ObservationSelection::absent(selector),
        )?;
        return Ok(ObserveOutput::Absent { context });
    };
    let candidate = snapshot.candidate(publication.run_id())?;
    let pinned = snapshot.pin(publication);
    let session = Session::sample(&candidate, &pinned)?;
    wait.checkpoint(
        ObservationStage::Selection,
        &ObservationSelection::pinned(&pinned),
    )?;
    Ok(ObserveOutput::Session { context, session })
}

pub fn await_selected(
    admission: &ControlAdmission,
    location: &SlotLocation,
    selector: &Selector,
    step: &str,
    budget: &WaitBudget,
    cancellation: &CancellationToken,
) -> RuntimeResult<TaskObservation> {
    let lexical_step = RootStepPath::parse(step).ok();
    let establishment = ObservationEstablishment::not_requested();
    let wait = ObservationWait {
        budget,
        cancellation,
        establishment: &establishment,
        step: lexical_step.as_ref(),
    };
    context(admission, location)?;
    let (snapshot, selected) = select(admission, location, selector, &wait)?;
    let Some(publication) = selected else {
        return Err(session_unavailable(
            SessionUnavailableReason::Absent,
            &ObservationSelection::absent(selector),
            lexical_step.as_ref(),
            None,
        ));
    };
    await_root(admission, location, snapshot.pin(publication), step, &wait)
}

pub(crate) fn compare_request(
    expected: &RecordedRunRequest,
    recorded: &RecordedRunRequest,
    selection: &ObservationSelection,
) -> RuntimeResult<()> {
    let mismatch = if expected.computed_manifest_hash() != recorded.computed_manifest_hash() {
        Some(RequestMismatchField::Manifest)
    } else if expected.root_task() != recorded.root_task() {
        Some(RequestMismatchField::RootTask)
    } else if expected.source() != recorded.source() {
        Some(RequestMismatchField::Source)
    } else if expected.operation_timeout_ms() != recorded.operation_timeout_ms() {
        Some(RequestMismatchField::OperationTimeout)
    } else {
        None
    };
    match mismatch {
        Some(field) => Err(request_mismatch(field, selection)),
        None => Ok(()),
    }
}

fn read_candidate(
    pinned: &mut PinnedRun,
    wait: &ObservationWait<'_>,
    poll: &mut impl FnMut(ObservationBoundary),
) -> RuntimeResult<CandidateSnapshot> {
    loop {
        poll(ObservationBoundary::Poll);
        wait.checkpoint(
            ObservationStage::Task,
            &ObservationSelection::pinned(pinned),
        )?;
        if let Some(snapshot) = pinned.read()? {
            return snapshot.candidate(pinned.run_id());
        }
        pause(wait.budget);
    }
}

fn validate_plan(
    candidate: &CandidateSnapshot,
    plan: &crate::execution::RunPlan<'_>,
) -> RuntimeResult<()> {
    for (path, process) in &candidate.roots {
        let leaf = plan
            .root_leaf(path)
            .map_err(|_| corrupt("root attribution leaves its bound execution plan"))?;
        if process.outcome == Some(ExecutionOutcome::Succeeded)
            && !process
                .exit
                .is_some_and(|exit| leaf.node().task().success_codes.contains(&exit))
        {
            return Err(corrupt(
                "succeeded task outcome has no policy-accepted exit",
            ));
        }
        if process.status == ProcessStatus::Succeeded
            && process
                .outcome
                .is_some_and(|outcome| outcome != ExecutionOutcome::Succeeded)
        {
            return Err(corrupt("succeeded task status contradicts its outcome"));
        }
    }
    for service in &candidate.services {
        let binding = plan
            .services()
            .iter()
            .find(|binding| binding.service().name.as_str() == service.service_id)
            .ok_or_else(|| corrupt("recorded service leaves the selected service group"))?;
        if binding.service().endpoints().count() != service.endpoints.len()
            || binding.service().endpoints().zip(&service.endpoints).any(
                |((id, endpoint), actual)| {
                    id.as_str() != actual.endpoint_id
                        || endpoint.host.to_string() != actual.host.as_str()
                        || binding.endpoint_ports()[id.as_str()] != actual.port
                },
            )
        {
            return Err(corrupt(
                "recorded endpoint set differs from the selected plan",
            ));
        }
    }
    if !candidate.roots.is_empty() && candidate.services.len() != plan.services().len() {
        return Err(corrupt(
            "root attribution exists before its complete service group",
        ));
    }
    Ok(())
}

fn successful_task(task: &ProcessFacts) -> bool {
    task.status == ProcessStatus::Succeeded
        && task.outcome == Some(ExecutionOutcome::Succeeded)
        && task.exit.is_some()
        && task.ownership == Ownership::Settled
        && task.capture == CaptureState::Finished(CaptureOutcome::Complete)
}

fn unavailable(
    candidate: &CandidateSnapshot,
    session: &Session,
    task: Option<&ProcessFacts>,
) -> Option<SessionUnavailableReason> {
    use SessionUnavailableReason as Reason;
    if candidate.successor || session.owner.observation == LeaderObservation::IdentityMismatch {
        return Some(Reason::OwnerReplaced);
    }
    if session.owner.observation == LeaderObservation::Absent {
        return Some(Reason::OwnerGone);
    }
    if !matches!(candidate.run.progress, SessionProgress::Executing) {
        return Some(Reason::ExecutionSettled);
    }
    if let Some(task) = task {
        if task
            .outcome
            .is_some_and(|outcome| outcome != ExecutionOutcome::Succeeded)
            || !matches!(
                task.status,
                ProcessStatus::Running | ProcessStatus::Succeeded
            )
        {
            return Some(Reason::TaskFailed);
        }
        if matches!(
            task.capture,
            CaptureState::Finished(CaptureOutcome::Incomplete | CaptureOutcome::Unknown)
        ) || (task.ownership == Ownership::Unresolved
            && matches!(task.capture, CaptureState::Finished(_)))
            || (task.status == ProcessStatus::Succeeded && !successful_task(task))
        {
            return Some(Reason::EvidenceIncomplete);
        }
    }
    if candidate.services.iter().any(|service| {
        service.process.ownership == Ownership::Settled
            || service.process.outcome.is_some()
            || !matches!(
                service.process.status,
                ProcessStatus::Running | ProcessStatus::Ready
            )
    }) {
        return Some(Reason::ServiceSettled);
    }
    for service in &session.services {
        match service.leader.observation {
            LeaderObservation::Absent => return Some(Reason::ServiceGone),
            LeaderObservation::IdentityMismatch => return Some(Reason::ServiceIdentityMismatch),
            LeaderObservation::Live => {}
        }
    }
    None
}

fn service_identities_unchanged(before: &[ServiceFacts], after: &[ServiceFacts]) -> bool {
    before.len() == after.len()
        && before.iter().zip(after).all(|(before, after)| {
            before.service_id == after.service_id
                && before.endpoints == after.endpoints
                && before.process.key == after.process.key
                && before.process.pid == after.process.pid
                && before.process.pgid == after.process.pgid
                && before.process.start == after.process.start
        })
}

pub fn await_root(
    admission: &ControlAdmission,
    location: &SlotLocation,
    pinned: PinnedRun,
    step: &str,
    wait: &ObservationWait<'_>,
) -> RuntimeResult<TaskObservation> {
    await_root_with_poll(admission, location, pinned, step, wait, |_| {})
}

#[derive(Clone, Copy)]
pub(crate) enum ObservationBoundary {
    Poll,
    LeadersSampled,
}

pub(crate) fn await_root_with_poll(
    admission: &ControlAdmission,
    location: &SlotLocation,
    mut pinned: PinnedRun,
    step: &str,
    wait: &ObservationWait<'_>,
    mut poll: impl FnMut(ObservationBoundary),
) -> RuntimeResult<TaskObservation> {
    let lexical_step = RootStepPath::parse(step).ok();
    let wait = ObservationWait {
        step: lexical_step.as_ref(),
        ..*wait
    };
    let context = context(admission, location)?;
    let scope = Scope::new(admission, location)?;
    if pinned.scope().location() != scope.location()
        || pinned.scope().identity() != scope.identity()
    {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            "pinned observation has a different admitted scope",
        ));
    }
    let selection = ObservationSelection::pinned(&pinned);
    let mut candidate = read_candidate(&mut pinned, &wait, &mut poll)?;
    if admission.computed_manifest_hash() != candidate.run.request.computed_manifest_hash() {
        return Err(request_mismatch(RequestMismatchField::Manifest, &selection));
    }
    let step = RootStepPath::parse(step)?;
    let plan = crate::execution::plan(
        admission.execution_manifest(),
        candidate.run.request.root_task(),
        location.slot().slot(),
    )
    .map_err(|_| corrupt("recorded root task cannot produce its admitted plan"))?;
    let leaf = plan.root_leaf(step.as_str())?;
    let immutable = candidate.run.clone();
    let placement = location.for_run(pinned.run_id())?;
    loop {
        poll(ObservationBoundary::Poll);
        validate_plan(&candidate, &plan)?;
        if candidate.run.request != immutable.request
            || candidate.run.owner != immutable.owner
            || candidate.run.manifest_path != immutable.manifest_path
        {
            return Err(corrupt("immutable selected run provenance changed"));
        }
        let mut session = Session::sample(&candidate, &pinned)?;
        let task = candidate.roots.get(step.as_str());
        let evidence = task
            .map(|task| ObservationEvidence::new(&placement, &task.stdout, &task.stderr))
            .transpose()?;
        if let Some(reason) = unavailable(&candidate, &session, task) {
            return Err(session_unavailable(
                reason,
                &selection,
                Some(&step),
                evidence.as_ref(),
            ));
        }
        let ready = task.is_some_and(successful_task)
            && candidate.services.len() == plan.services().len()
            && candidate
                .services
                .iter()
                .all(|service| service.process.status == ProcessStatus::Ready);
        if ready {
            poll(ObservationBoundary::LeadersSampled);
            let fresh = read_candidate(&mut pinned, &wait, &mut poll)?;
            validate_plan(&fresh, &plan)?;
            if fresh.run.request != immutable.request
                || fresh.run.owner != immutable.owner
                || fresh.run.manifest_path != immutable.manifest_path
                || !service_identities_unchanged(&candidate.services, &fresh.services)
                || fresh.roots.get(step.as_str()) != task
            {
                return Err(corrupt(
                    "canonical selected evidence changed during leader sampling",
                ));
            }
            if let Some(reason) = unavailable(&fresh, &session, fresh.roots.get(step.as_str())) {
                return Err(session_unavailable(
                    reason,
                    &selection,
                    Some(&step),
                    evidence.as_ref(),
                ));
            }
            if fresh
                .services
                .iter()
                .any(|service| service.process.status != ProcessStatus::Ready)
            {
                return Err(corrupt(
                    "ready service evidence regressed during observation",
                ));
            }
            session.owner = Leader::owner(&fresh.run.owner)?;
            if session.owner.observation != LeaderObservation::Live {
                let reason = match session.owner.observation {
                    LeaderObservation::IdentityMismatch => SessionUnavailableReason::OwnerReplaced,
                    LeaderObservation::Absent => SessionUnavailableReason::OwnerGone,
                    LeaderObservation::Live => unreachable!(),
                };
                return Err(session_unavailable(
                    reason,
                    &selection,
                    Some(&step),
                    evidence.as_ref(),
                ));
            }
            wait.checkpoint(ObservationStage::Task, &selection)?;
            let task = fresh
                .roots
                .get(step.as_str())
                .expect("fresh canonical root was checked");
            return Ok(TaskObservation {
                context,
                session,
                task: CompletedRootTask {
                    step_path: step.as_str().to_owned(),
                    task_id: leaf.node().task().task_id.to_string(),
                    process_key: task.key.clone(),
                    exit_code: task.exit.expect("successful task has an exit"),
                    stdout_path: utf8_locator(&task.stdout.in_run(&placement), "stdoutPath")?
                        .to_owned(),
                    stderr_path: utf8_locator(&task.stderr.in_run(&placement), "stderrPath")?
                        .to_owned(),
                },
            });
        }
        pause(wait.budget);
        candidate = read_candidate(&mut pinned, &wait, &mut poll)?;
    }
}

fn corrupt(message: &'static str) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use crate::registry::session::{
        DeclaredRunRequest, record_execution_outcome, record_run_created,
    };
    use crate::state::ownership::SlotGuard;
    use nixfied_manifest::fixtures::{SyntheticManifestOptions, synthetic_manifest};
    use serde_json::json;

    #[test]
    fn final_reread_rejects_settlement_or_successor_after_leader_sampling() {
        for (mutation, reason) in [
            ("none", None),
            ("settled", Some("execution-settled")),
            ("successor", Some("owner-replaced")),
        ] {
            let root = crate::test_support::TestDir::new("observation-final-reread");
            let executable = std::env::var("NIXFIED_TEST_CHILD").unwrap();
            let mut manifest = synthetic_manifest(&SyntheticManifestOptions {
                executable: executable.clone(),
                ..Default::default()
            });
            manifest["tasks"]["smoke"]["requires"] = json!([]);
            manifest["tasks"]["smoke"]["invocation"]["run"] = json!([
                std::path::Path::new(&executable)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "exit",
                "0"
            ]);
            let admission = crate::test_support::admit_manifest(&root, &manifest);
            let location = crate::state::derive_slot_location(
                admission.common().project_id(),
                "dev",
                0,
                root.join("state"),
            )
            .unwrap();
            let scope = Scope::new(admission.common(), &location).unwrap();
            let mut writer = Registry::open_or_create(
                SlotGuard::acquire(&location, &CancellationToken::new()).unwrap(),
                scope.identity(),
            )
            .unwrap();
            let request =
                DeclaredRunRequest::new(&admission, &nixfied_manifest::TaskId::new("smoke"), 5000)
                    .unwrap();
            record_run_created(
                &mut writer,
                "selected",
                &request,
                &location.for_run("selected").unwrap(),
                None,
            )
            .unwrap();
            let owner_json: String = writer
                .connection()
                .query_row("SELECT owner_identity FROM runs", [], |row| row.get(0))
                .unwrap();
            let owner: OwnerIdentity = serde_json::from_str(&owner_json).unwrap();
            writer.connection().execute(
                "INSERT INTO processes(process_key,run_id,pid,pgid,start_identity,command_json,role,root_step_path,execution_outcome,exit_code,status,ownership,source_label,presentation,stdout_path,stderr_path,capture,stop_signal,stop_timeout_ms,containment)
                 VALUES ('task','selected',?1,?1,?2,'{}','task','smoke','succeeded',0,'succeeded','settled','smoke','hidden','logs/out.log','logs/err.log','complete',15,1000,'process-group')",
                rusqlite::params![owner.pid(), owner_json],
            ).unwrap();
            let snapshot = Snapshot::read(&scope, &mut History::default())
                .unwrap()
                .unwrap();
            let publication = snapshot.publication("selected").unwrap().clone();
            let pinned = snapshot.pin(publication);
            let mut sampled = false;
            let result = await_root_with_poll(
                admission.common(),
                &location,
                pinned,
                "smoke",
                &ObservationWait {
                    budget: &WaitBudget::new(std::num::NonZeroU64::new(1000).unwrap()),
                    cancellation: &CancellationToken::new(),
                    establishment: &ObservationEstablishment::not_requested(),
                    step: None,
                },
                |boundary| {
                    if !matches!(boundary, ObservationBoundary::LeadersSampled) || sampled {
                        return;
                    }
                    sampled = true;
                    match mutation {
                        "none" => {}
                        "settled" => record_execution_outcome(
                            &mut writer,
                            "selected",
                            admission.common().computed_manifest_hash(),
                            ExecutionOutcome::Failed,
                        )
                        .unwrap(),
                        "successor" => record_run_created(
                            &mut writer,
                            "successor",
                            &request,
                            &location.for_run("successor").unwrap(),
                            None,
                        )
                        .unwrap(),
                        _ => unreachable!(),
                    }
                },
            );
            assert!(sampled, "the wait must reach exact leader sampling");
            if let Some(reason) = reason {
                let error = result.unwrap_err();
                assert_eq!(error.code, ErrorCode::SessionUnavailable);
                assert_eq!(error.details["sessionObservation"]["reason"], reason);
                assert_eq!(
                    error.details["sessionObservation"]["selection"],
                    json!({"state":"pinned","runId":"selected"})
                );
            } else {
                assert_eq!(
                    result.unwrap().session.owner.observation,
                    LeaderObservation::Live
                );
            }
            writer.close().unwrap();
        }
    }
}
