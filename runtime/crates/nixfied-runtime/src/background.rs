//! Background placement of one ordinary task session.
//!
//! The launcher owns no slot guard or registry writer. It validates the run
//! request, allocates the immutable run identity, spawns the owner once in a
//! new OS session with null stdio, and sends one bounded request over a private
//! socketpair. The owner runs the same admission, acquisition, recovery, and
//! establishment as a foreground run; immediately before committing its run
//! record it checks whether the launcher abandoned startup. The committed run
//! record is the establishment: the owner then replies with the run identity
//! and evidence location and continues independently. Acknowledgement never
//! means readiness or task success.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cancellation::{CancellationToken, WaitBudget};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::error::{
    ObservationEstablishment, ObservationSelection, ObservationStage, bootstrap_uncertain,
    observation_checkpoint,
};
use crate::registry::observation::{CursorWire, Seeking};
use crate::registry::session::RecordedRunRequestWire;
use crate::registry::session::valid_run_id;

pub const COMMAND: &str = "__session-owner";
const PROTOCOL: crate::channel::Protocol = crate::channel::Protocol {
    magic: *b"NXD2",
    max: 1024 * 1024,
};
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const REPLY_TIMEOUT: Duration = Duration::from_secs(1);

/// The launcher's validated request: forwarded run arguments and the
/// immutable run identity it allocated before spawning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "purpose", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    Daemon {
        #[serde(rename = "runId")]
        run_id: String,
        args: Vec<String>,
    },
    Shared {
        #[serde(rename = "runId")]
        run_id: String,
        execution: Execution,
        #[serde(rename = "expectedRequest")]
        expected_request: Box<RecordedRunRequestWire>,
        cursor: CursorWire,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Execution {
    pub manifest_path: String,
    pub allow_non_store_manifest: bool,
    pub task: String,
    pub slot: u32,
    pub state_base: String,
    pub operation_timeout_ms: u64,
}

impl Request {
    fn checked(&self) -> RuntimeResult<()> {
        let run_id = match self {
            Self::Daemon { run_id, .. } => run_id,
            Self::Shared {
                run_id,
                execution,
                expected_request,
                cursor,
            } => {
                for path in [&execution.manifest_path, &execution.state_base] {
                    if path.contains('\0') || !Path::new(path).is_absolute() {
                        return Err(transport_failure("invalid shared execution locator"));
                    }
                }
                if !nixfied_manifest::validation::is_step_safe_id(&execution.task) {
                    return Err(transport_failure("invalid shared execution task"));
                }
                expected_request.checked()?;
                cursor.checked()?;
                run_id
            }
        };
        if !valid_run_id(run_id) {
            return Err(transport_failure("invalid bootstrap run identity"));
        }
        Ok(())
    }
}

/// An established session: its immutable identity and retained evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Acknowledgement {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub logs_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "result", deny_unknown_fields)]
enum DaemonReply {
    Established(Acknowledgement),
    Rejected(Rejection),
}

/// A redaction-safe pre-establishment failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Rejection {
    code: ErrorCode,
    message: String,
    details: Value,
    causes: Vec<CauseWire>,
}

/// The bounded, nonrecursive cause projection has its own input DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CauseWire {
    code: ErrorCode,
    #[serde(deserialize_with = "cause_exit_class")]
    exit_class: crate::error::ExitClass,
    message: String,
    details: Value,
}

fn cause_exit_class<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<crate::error::ExitClass, D::Error> {
    let value = String::deserialize(deserializer)?;
    for class in [crate::error::ExitClass::Ok, crate::error::ExitClass::Error] {
        if serde_json::to_value(class)
            .ok()
            .as_ref()
            .and_then(Value::as_str)
            == Some(value.as_str())
        {
            return Ok(class);
        }
    }
    Err(serde::de::Error::custom("invalid cause exit class"))
}

impl Rejection {
    fn from_error(error: &RuntimeError) -> Self {
        Self {
            code: error.code,
            message: error.message.clone(),
            details: error.details.clone(),
            causes: error
                .causes
                .iter()
                .map(|cause| CauseWire {
                    code: cause.code,
                    exit_class: cause.exit_class,
                    message: cause.message.clone(),
                    details: cause.details.clone(),
                })
                .collect(),
        }
    }
    fn into_error(self) -> RuntimeError {
        let mut error = RuntimeError::new(self.code, self.message).with_details(self.details);
        error.causes.extend(
            self.causes
                .into_iter()
                .map(|cause| crate::error::RuntimeCause {
                    code: cause.code,
                    exit_class: cause.exit_class,
                    message: cause.message,
                    details: cause.details,
                }),
        );
        error
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case", deny_unknown_fields)]
enum SharedReply {
    Established {
        #[serde(rename = "runId")]
        run_id: String,
        #[serde(rename = "runDir")]
        run_dir: PathBuf,
        #[serde(rename = "logsDir")]
        logs_dir: PathBuf,
        cursor: CursorWire,
    },
    SelectedExisting {
        #[serde(rename = "runId")]
        run_id: String,
        #[serde(rename = "createdSeq")]
        created_sequence: i64,
        cursor: CursorWire,
    },
    Rejected(Rejection),
}

/// Selection metadata is not evidence of usability. The facade rereads the
/// first canonical publication before pinning it.
pub enum SharedSelection {
    Established {
        acknowledgement: Acknowledgement,
        cursor: CursorWire,
    },
    Existing {
        run_id: String,
        created_sequence: std::num::NonZeroI64,
        cursor: CursorWire,
    },
}

pub struct SharedLaunch {
    pub selection: SharedSelection,
    helper: Option<Child>,
}

impl SharedLaunch {
    pub fn reap_exited(&mut self) {
        if let Some(helper) = &mut self.helper
            && matches!(helper.try_wait(), Ok(Some(_)))
        {
            self.helper = None;
        }
    }
    pub fn await_root(
        mut self,
        admission: &crate::admission::ControlAdmission,
        location: &crate::state::SlotLocation,
        pinned: crate::registry::observation::PinnedRun,
        step: &str,
        budget: &WaitBudget,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<crate::control::observation::TaskObservation> {
        crate::control::observation::await_root_with_poll(
            admission,
            location,
            pinned,
            step,
            budget,
            cancellation,
            || self.reap_exited(),
        )
    }
}

fn transport_failure(message: &'static str) -> RuntimeError {
    RuntimeError::new(ErrorCode::LifecycleFailed, message)
}

/// What the launcher observed; only a complete valid reply is conclusive.
#[derive(Debug)]
pub enum LaunchOutcome {
    Established(Acknowledgement),
    Rejected(RuntimeError),
    /// EOF, timeout, malformed reply, or owner death without a conclusive
    /// reply: the session may or may not have been established.
    Uncertain,
    /// The launcher was interrupted before a conclusive reply; completeness
    /// of the owner's abandonment is unknown.
    Interrupted,
    /// Establishment won the race with the launcher's interruption; the
    /// launcher then requested cancellation of exactly that session.
    CanceledAfterEstablishment(Acknowledgement),
    /// Establishment won the race with the launcher's interruption, but the
    /// cancellation request did not reach that session's owner.
    CancellationUndelivered {
        acknowledgement: Acknowledgement,
        cause: RuntimeError,
    },
}

/// Apply launcher interruption without losing rejection or delivery evidence.
fn interrupt(outcome: LaunchOutcome) -> LaunchOutcome {
    use crate::session_control::{CancellationDelivery, request_cancellation};
    let acknowledgement = match outcome {
        LaunchOutcome::Established(acknowledgement) => acknowledgement,
        LaunchOutcome::Rejected(error) => {
            return LaunchOutcome::Rejected(
                crate::cancellation::canceled_error().with_cause(error),
            );
        }
        outcome => return outcome,
    };
    match request_cancellation(&acknowledgement.run_dir) {
        Ok(CancellationDelivery::Requested) => {
            LaunchOutcome::CanceledAfterEstablishment(acknowledgement)
        }
        Ok(CancellationDelivery::Unavailable) => LaunchOutcome::CancellationUndelivered {
            acknowledgement,
            cause: RuntimeError::new(
                ErrorCode::LifecycleFailed,
                "the established session's owner no longer listens for cancellation",
            ),
        },
        Err(cause) => LaunchOutcome::CancellationUndelivered {
            acknowledgement,
            cause,
        },
    }
}

/// After an interruption the launcher waits this long for a conclusive reply.
const INTERRUPT_GRACE: Duration = Duration::from_secs(10);

/// Spawn the owner, send the request, and wait for one bounded reply. The
/// launcher keeps its end open while waiting: closing it means abandonment.
pub fn launch(runtime: &Path, request: &Request, wait: Duration) -> RuntimeResult<LaunchOutcome> {
    if !matches!(request, Request::Daemon { .. }) {
        return Err(transport_failure("ordinary launch requires daemon purpose"));
    }
    request.checked()?;
    let failure = |message: &str| RuntimeError::new(ErrorCode::LifecycleFailed, message);
    let body = serde_json::to_vec(request).map_err(|_| failure("cannot encode launch request"))?;
    let frame = PROTOCOL
        .encode(&body)
        .map_err(|_| failure("background launch request exceeds its limit"))?;
    let (mut channel, mut owner) = spawn_owner(runtime)?;
    if crate::channel::write_all(&mut channel, &frame, Instant::now() + REQUEST_TIMEOUT).is_err() {
        let _ = owner.kill();
        let _ = owner.wait();
        return Err(failure("cannot deliver the background launch request"));
    }
    let mut deadline = Instant::now() + wait;
    let mut reader = crate::channel::FrameReader::new(&PROTOCOL);
    let mut interrupted = false;
    let outcome = loop {
        if !interrupted && crate::cancellation::signal_received() {
            // Half-closing is the abandonment signal; the reply side stays
            // open so a won race is still observed and canceled precisely.
            interrupted = true;
            let _ = channel.shutdown(std::net::Shutdown::Write);
            deadline = Instant::now() + INTERRUPT_GRACE;
        }
        match reader.step(&mut channel) {
            Ok(Some(body)) => {
                break match serde_json::from_slice::<DaemonReply>(&body) {
                    Ok(DaemonReply::Established(acknowledgement))
                        if valid_ack(&acknowledgement) =>
                    {
                        LaunchOutcome::Established(acknowledgement)
                    }
                    Ok(DaemonReply::Rejected(rejection)) => {
                        LaunchOutcome::Rejected(rejection.into_error())
                    }
                    _ => LaunchOutcome::Uncertain,
                };
            }
            Ok(None) if Instant::now() < deadline => {
                let _ = crate::channel::wait(&channel, libc::POLLIN, deadline);
            }
            Ok(None) | Err(_) if interrupted => break LaunchOutcome::Interrupted,
            Ok(None) | Err(_) => break LaunchOutcome::Uncertain,
        }
    };
    // A rejected, abandoned, or already-exited owner is reaped here; an
    // established owner continues and is adopted by the host when the
    // launcher exits.
    if matches!(
        outcome,
        LaunchOutcome::Rejected(_) | LaunchOutcome::Uncertain | LaunchOutcome::Interrupted
    ) {
        let give_up = Instant::now() + REPLY_TIMEOUT;
        while Instant::now() < give_up {
            if owner.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
    // Include signals observed while reaping a rejected owner.
    Ok(if interrupted || crate::cancellation::signal_received() {
        interrupt(outcome)
    } else {
        outcome
    })
}

fn valid_ack(ack: &Acknowledgement) -> bool {
    valid_run_id(&ack.run_id)
        && [&ack.run_dir, &ack.logs_dir].iter().all(|path| {
            path.is_absolute() && path.to_str().is_some_and(|text| !text.contains('\0'))
        })
}

fn spawn_owner(runtime: &Path) -> RuntimeResult<(UnixStream, Child)> {
    use std::os::unix::process::CommandExt;
    let (channel, child_channel) = crate::channel::pair()
        .map_err(|_| transport_failure("cannot create background launch channel"))?;
    let mut command = Command::new(runtime);
    command.arg(COMMAND);
    crate::channel::inherit(&mut command, &child_channel, &channel);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and detaches the controlling terminal.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let owner = crate::spawn::command(&mut command)
        .map_err(|_| transport_failure("cannot spawn background session owner"))?;
    drop(child_channel);
    Ok((channel, owner))
}

/// Full frame delivery is the only boundary after which this observer must
/// never terminate the helper. The elapsed budget covers writes and replies.
pub fn launch_shared(
    runtime: &Path,
    request: &Request,
    seeking: &Seeking,
    step: &crate::control::observation::RootStepPath,
    budget: &WaitBudget,
    cancellation: &CancellationToken,
) -> RuntimeResult<SharedLaunch> {
    let Request::Shared { run_id, .. } = request else {
        return Err(transport_failure("shared launch requires shared purpose"));
    };
    request.checked()?;
    let body = serde_json::to_vec(request)
        .map_err(|_| transport_failure("cannot encode shared launch request"))?;
    let frame = PROTOCOL
        .encode(&body)
        .map_err(|_| transport_failure("shared launch request exceeds its limit"))?;
    let selection = ObservationSelection::seeking(seeking);
    let (mut channel, mut helper) = spawn_owner(runtime)?;
    let result = shared_exchange(
        &mut channel,
        &frame,
        run_id,
        &selection,
        step,
        budget,
        cancellation,
    );
    let _ = channel.shutdown(std::net::Shutdown::Write);
    match result {
        Exchange::Incomplete(error) => {
            drop(channel);
            let _ = helper.kill();
            let _ = helper.wait();
            Err(error)
        }
        Exchange::Complete(result) => {
            // No grace, kill or FIFO cancellation after full dispatch.
            let exited = matches!(helper.try_wait(), Ok(Some(_)));
            result.map(|selection| {
                let keep = matches!(selection, SharedSelection::Existing { .. }) && !exited;
                SharedLaunch {
                    selection,
                    helper: keep.then_some(helper),
                }
            })
        }
    }
}

enum Exchange {
    Incomplete(RuntimeError),
    Complete(RuntimeResult<SharedSelection>),
}

fn shared_exchange(
    channel: &mut UnixStream,
    frame: &[u8],
    candidate: &str,
    selection: &ObservationSelection,
    step: &crate::control::observation::RootStepPath,
    budget: &WaitBudget,
    cancellation: &CancellationToken,
) -> Exchange {
    let not_established = match ObservationEstablishment::not_established(candidate) {
        Ok(value) => value,
        Err(error) => return Exchange::Incomplete(error),
    };
    let checkpoint = |establishment: &ObservationEstablishment| {
        observation_checkpoint(
            budget,
            cancellation,
            ObservationStage::Establishment,
            selection,
            establishment,
            Some(step),
        )
    };
    let mut written = 0;
    while written < frame.len() {
        if let Err(error) = checkpoint(&not_established) {
            return Exchange::Incomplete(error);
        }
        match channel.write(&frame[written..]) {
            Ok(0) => {
                return Exchange::Incomplete(transport_failure(
                    "shared request channel closed during delivery",
                ));
            }
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if let Some(deadline) = budget.poll_deadline() {
                    let _ = crate::channel::wait(channel, libc::POLLOUT, deadline);
                }
            }
            Err(_) => {
                return Exchange::Incomplete(transport_failure("cannot deliver shared request"));
            }
        }
    }
    let uncertain = ObservationEstablishment::uncertain(candidate).expect("candidate checked");
    let mut reader = crate::channel::FrameReader::new(&PROTOCOL);
    loop {
        if let Err(error) = checkpoint(&uncertain) {
            return Exchange::Complete(Err(error));
        }
        match reader.step(channel) {
            Ok(Some(body)) => {
                let reply = decode_shared_reply(&body, candidate);
                let certainty = match &reply {
                    Ok(Ok(SharedSelection::Established {
                        acknowledgement, ..
                    })) => ObservationEstablishment::established(&acknowledgement.run_id)
                        .expect("reply checked"),
                    Ok(_) => not_established.clone(),
                    Err(_) => uncertain.clone(),
                };
                if let Err(error) = checkpoint(&certainty) {
                    return Exchange::Complete(Err(error));
                }
                return Exchange::Complete(
                    reply.unwrap_or_else(|_| Err(bootstrap_uncertain(selection, &uncertain))),
                );
            }
            Ok(None) => {
                if let Some(deadline) = budget.poll_deadline() {
                    let _ = crate::channel::wait(channel, libc::POLLIN, deadline);
                }
            }
            Err(_) => {
                if let Err(error) = checkpoint(&uncertain) {
                    return Exchange::Complete(Err(error));
                }
                return Exchange::Complete(Err(bootstrap_uncertain(selection, &uncertain)));
            }
        }
    }
}

fn decode_shared_reply(body: &[u8], candidate: &str) -> Result<RuntimeResult<SharedSelection>, ()> {
    let reply: SharedReply = serde_json::from_slice(body).map_err(|_| ())?;
    Ok(Ok(match reply {
        SharedReply::Established {
            run_id,
            run_dir,
            logs_dir,
            cursor,
        } => {
            let acknowledgement = Acknowledgement {
                run_id,
                run_dir,
                logs_dir,
            };
            if acknowledgement.run_id != candidate || !valid_ack(&acknowledgement) {
                return Err(());
            }
            cursor.checked().map_err(|_| ())?;
            SharedSelection::Established {
                acknowledgement,
                cursor,
            }
        }
        SharedReply::SelectedExisting {
            run_id,
            created_sequence,
            cursor,
        } => {
            if !valid_run_id(&run_id) || created_sequence <= 0 {
                return Err(());
            }
            cursor.checked().map_err(|_| ())?;
            SharedSelection::Existing {
                run_id,
                created_sequence: created_sequence.try_into().map_err(|_| ())?,
                cursor,
            }
        }
        SharedReply::Rejected(rejection) => return Ok(Err(rejection.into_error())),
    }))
}

/// The owner's end of the channel between request and establishment.
pub struct Establishment {
    channel: UnixStream,
    purpose: Purpose,
    phase: Phase,
}

enum Purpose {
    Daemon,
    Shared(CursorWire),
}
#[derive(PartialEq, Eq)]
enum Phase {
    Precommit,
    Committed,
    Replied,
}

/// Every execution mode enters the same owned executor. Detached purpose is
/// explicit; its transport owns the corresponding closed request purpose.
pub enum SessionPurpose<'a> {
    Foreground,
    Detached(&'a mut Establishment),
}

impl SessionPurpose<'_> {
    pub fn is_background(&self) -> bool {
        !matches!(self, Self::Foreground)
    }
    pub fn check_abandonment(&mut self) -> RuntimeResult<()> {
        match self {
            Self::Foreground => Ok(()),
            Self::Detached(channel) => channel.check_abandonment(),
        }
    }
    pub fn mark_committed(&mut self) {
        match self {
            Self::Foreground => {}
            Self::Detached(channel) => channel.mark_committed(),
        }
    }
    pub fn acknowledge(
        &mut self,
        acknowledgement: Acknowledgement,
        admission: &crate::admission::ControlAdmission,
        location: &crate::state::SlotLocation,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<()> {
        match self {
            Self::Foreground => Ok(()),
            Self::Detached(channel) => {
                channel.acknowledge(acknowledgement, admission, location, cancellation)
            }
        }
    }
}

/// Return None for ordinary runtime commands. Otherwise decode the single
/// bounded request before any ordinary runtime initialization.
pub fn receive(args: &[OsString]) -> Option<Result<(Request, Establishment), i32>> {
    let failure = Some(Err(crate::channel::FAILURE_EXIT));
    let mut channel = match crate::channel::admit(args, COMMAND)? {
        Ok(channel) => channel,
        Err(exit) => return Some(Err(exit)),
    };
    let Ok(body) = PROTOCOL.read(&mut channel, Instant::now() + REQUEST_TIMEOUT) else {
        return failure;
    };
    let Ok(request) = serde_json::from_slice::<Request>(&body) else {
        return failure;
    };
    if request.checked().is_err() {
        return failure;
    }
    let purpose = match &request {
        Request::Daemon { .. } => Purpose::Daemon,
        Request::Shared { cursor, .. } => Purpose::Shared(cursor.clone()),
    };
    Some(Ok((
        request,
        Establishment {
            channel,
            purpose,
            phase: Phase::Precommit,
        },
    )))
}

impl Establishment {
    #[cfg(test)]
    pub(crate) fn shared_fixture(cursor: CursorWire) -> (UnixStream, Self) {
        let (launcher, channel) = crate::channel::pair().unwrap();
        (
            launcher,
            Self {
                channel,
                purpose: Purpose::Shared(cursor),
                phase: Phase::Precommit,
            },
        )
    }
    /// Observed startup abandonment: the launcher closed its end before the
    /// owner committed. Data after the request is also a protocol violation.
    pub fn check_abandonment(&mut self) -> RuntimeResult<()> {
        if self.phase != Phase::Precommit {
            return Err(transport_failure(
                "abandonment checked after run commitment",
            ));
        }
        let mut byte = [0_u8; 1];
        match self.channel.read(&mut byte) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
            _ => Err(RuntimeError::new(
                ErrorCode::Canceled,
                "the background launcher abandoned startup before establishment",
            )),
        }
    }

    /// Reply after the committed establishment. A failed reply never cancels
    /// the established session.
    pub fn learn_shared_cursor(&mut self, cursor: CursorWire) -> RuntimeResult<()> {
        match (&mut self.purpose, &self.phase) {
            (Purpose::Shared(previous), Phase::Precommit) => previous.advance_to(&cursor),
            _ => Err(transport_failure("cursor update outside shared precommit")),
        }
    }

    pub fn mark_committed(&mut self) {
        assert!(self.phase == Phase::Precommit, "run commitment occurs once");
        self.phase = Phase::Committed;
    }

    pub fn acknowledge(
        &mut self,
        acknowledgement: Acknowledgement,
        admission: &crate::admission::ControlAdmission,
        location: &crate::state::SlotLocation,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<()> {
        if self.phase != Phase::Committed {
            return Err(transport_failure("acknowledgement precedes commitment"));
        }
        let body = match &self.purpose {
            Purpose::Daemon => serde_json::to_vec(&DaemonReply::Established(acknowledgement)),
            Purpose::Shared(cursor) => {
                let cursor = crate::control::ensure::committed_cursor(
                    admission,
                    location,
                    cursor,
                    &acknowledgement.run_id,
                    cancellation,
                )?;
                serde_json::to_vec(&SharedReply::Established {
                    run_id: acknowledgement.run_id,
                    run_dir: acknowledgement.run_dir,
                    logs_dir: acknowledgement.logs_dir,
                    cursor,
                })
            }
        };
        self.write_reply(body);
        Ok(())
    }

    pub fn selected(
        &mut self,
        pinned: &crate::registry::observation::PinnedRun,
    ) -> RuntimeResult<()> {
        if !matches!(
            (&self.purpose, &self.phase),
            (Purpose::Shared(_), Phase::Precommit)
        ) {
            return Err(transport_failure("selection outside shared precommit"));
        }
        self.write_reply(serde_json::to_vec(&SharedReply::SelectedExisting {
            run_id: pinned.run_id().to_owned(),
            created_sequence: pinned.created_sequence(),
            cursor: pinned.cursor(),
        }));
        Ok(())
    }

    fn write_reply(&mut self, body: serde_json::Result<Vec<u8>>) {
        self.phase = Phase::Replied;
        if let Ok(body) = body {
            let _ = PROTOCOL.write(&mut self.channel, &body, Instant::now() + REPLY_TIMEOUT);
        }
    }

    /// Report a pre-establishment failure; after acknowledgement, failures
    /// belong to the session's own recorded outcome.
    pub fn reject(&mut self, error: &RuntimeError) {
        if self.phase != Phase::Precommit {
            return;
        }
        let rejection = Rejection::from_error(error);
        let body = match &self.purpose {
            Purpose::Daemon => serde_json::to_vec(&DaemonReply::Rejected(rejection)),
            Purpose::Shared(_) => serde_json::to_vec(&SharedReply::Rejected(rejection)),
        };
        self.write_reply(body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor() -> CursorWire {
        serde_json::from_value(serde_json::json!({"watermark":0,"history":{"state":"unseen"}}))
            .unwrap()
    }

    #[test]
    fn nxd2_requests_and_replies_are_closed_and_purpose_specific() {
        let daemon = serde_json::json!({"purpose":"daemon","runId":"run-1","args":[]});
        assert!(
            serde_json::from_value::<Request>(daemon.clone())
                .unwrap()
                .checked()
                .is_ok()
        );
        for raw in [
            serde_json::json!({"runId":"run-1","args":[]}),
            serde_json::json!({"purpose":"daemon","runId":"run-1","args":[],"cursor":cursor()}),
            serde_json::json!({"purpose":"daemon","runId":"run-1","args":null}),
        ] {
            assert!(serde_json::from_value::<Request>(raw).is_err());
        }
        let shared = serde_json::json!({"result":"established","runId":"run-1","runDir":"/r","logsDir":"/r/logs","cursor":cursor()});
        assert!(decode_shared_reply(&serde_json::to_vec(&shared).unwrap(), "run-1").is_ok());
        assert!(serde_json::from_value::<DaemonReply>(shared).is_err());
        for raw in [
            serde_json::json!({"result":"established","runId":"run-1","runDir":"/r","logsDir":"/r/logs"}),
            serde_json::json!({"result":"selected-existing","runId":"run-1","createdSeq":0,"cursor":cursor()}),
            serde_json::json!({"result":"selected-existing","runId":"../bad","createdSeq":1,"cursor":cursor()}),
            serde_json::json!({"result":"rejected","code":"TASK_FAILED","message":"safe","details":{},"causes":[{"code":"CANCELED","exitClass":"error","message":"cause","details":{},"causes":[]}]}),
        ] {
            assert!(decode_shared_reply(&serde_json::to_vec(&raw).unwrap(), "run-1").is_err());
        }
        let original = RuntimeError::new(ErrorCode::RequestMismatch, "safe").with_cause(
            RuntimeError::new(ErrorCode::RegistryCorrupt, "unsafe OS error"),
        );
        let raw =
            serde_json::to_vec(&SharedReply::Rejected(Rejection::from_error(&original))).unwrap();
        let Ok(Err(decoded)) = decode_shared_reply(&raw, "run-1") else {
            panic!("lost safe rejection");
        };
        assert_eq!(decoded.code, ErrorCode::RequestMismatch);
        assert_eq!(decoded.causes[0].code, ErrorCode::RegistryCorrupt);
        assert!(
            !serde_json::to_string(&decoded)
                .unwrap()
                .contains("unsafe OS error")
        );
    }

    #[test]
    fn incomplete_frame_stall_and_complete_frame_timeout_have_different_certainty() {
        let step = crate::control::observation::RootStepPath::parse("owner.init").unwrap();
        for complete in [false, true] {
            let (mut launcher, peer) = crate::channel::pair().unwrap();
            let frame = if complete {
                PROTOCOL.encode(b"{}").unwrap()
            } else {
                PROTOCOL.encode(&vec![b'x'; PROTOCOL.max]).unwrap()
            };
            let result = shared_exchange(
                &mut launcher,
                &frame,
                "run-1",
                &ObservationSelection::unenrolled(),
                &step,
                &WaitBudget::new(std::num::NonZeroU64::new(20).unwrap()),
                &CancellationToken::new(),
            );
            let error = match result {
                Exchange::Incomplete(error) if !complete => error,
                Exchange::Complete(Err(error)) if complete => error,
                _ => panic!("delivery certainty was not preserved"),
            };
            assert_eq!(error.code, ErrorCode::ObservationTimeout);
            assert_eq!(
                error.details["sessionObservation"]["stage"],
                "establishment"
            );
            assert_eq!(
                error.details["sessionObservation"]["establishment"]["state"],
                if complete {
                    "uncertain"
                } else {
                    "not-established"
                }
            );
            drop(peer);
        }
    }

    #[test]
    fn full_dispatch_malformed_or_eof_is_uncertain_without_waiting_for_grace() {
        for malformed in [false, true] {
            let (mut launcher, mut peer) = crate::channel::pair().unwrap();
            let server = thread::spawn(move || {
                PROTOCOL
                    .read(&mut peer, Instant::now() + REQUEST_TIMEOUT)
                    .unwrap();
                if malformed {
                    PROTOCOL
                        .write(&mut peer, b"{}", Instant::now() + REPLY_TIMEOUT)
                        .unwrap();
                }
            });
            let result = shared_exchange(
                &mut launcher,
                &PROTOCOL.encode(b"{}").unwrap(),
                "run-1",
                &ObservationSelection::unenrolled(),
                &crate::control::observation::RootStepPath::parse("owner.init").unwrap(),
                &WaitBudget::new(std::num::NonZeroU64::new(1000).unwrap()),
                &CancellationToken::new(),
            );
            let Exchange::Complete(Err(error)) = result else {
                panic!("lost full delivery");
            };
            assert_eq!(error.code, ErrorCode::LifecycleFailed);
            assert_eq!(
                error.details["sessionObservation"]["kind"],
                "bootstrap-uncertain"
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn observer_cancellation_between_commit_and_ack_does_not_reopen_precommit_rejection() {
        let (mut launcher, mut peer) = crate::channel::pair().unwrap();
        let canceled = CancellationToken::new();
        let observer = canceled.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            PROTOCOL
                .read(&mut peer, Instant::now() + REQUEST_TIMEOUT)
                .unwrap();
            let mut owner = Establishment {
                channel: peer,
                purpose: Purpose::Shared(cursor()),
                phase: Phase::Precommit,
            };
            owner.mark_committed();
            observer.cancel();
            wait.recv().unwrap();
            // A failure before any ACK is still postcommit and cannot produce
            // a precommit rejection. The owner remains free to execute.
            owner.reject(&RuntimeError::new(ErrorCode::TaskFailed, "postcommit"));
            assert!(owner.phase == Phase::Committed);
        });
        let result = shared_exchange(
            &mut launcher,
            &PROTOCOL.encode(b"{}").unwrap(),
            "run-1",
            &ObservationSelection::unenrolled(),
            &crate::control::observation::RootStepPath::parse("owner.init").unwrap(),
            &WaitBudget::new(std::num::NonZeroU64::new(1000).unwrap()),
            &canceled,
        );
        let Exchange::Complete(Err(error)) = result else {
            panic!("lost dispatch completion");
        };
        assert_eq!(error.code, ErrorCode::Canceled);
        assert_eq!(
            error.details["sessionObservation"]["establishment"]["state"],
            "uncertain"
        );
        drop(launcher);
        release.send(()).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn launcher_cancellation_reports_whether_it_reached_the_owner() {
        use std::os::unix::fs::DirBuilderExt;
        let LaunchOutcome::Rejected(error) = interrupt(LaunchOutcome::Rejected(RuntimeError::new(
            ErrorCode::ManifestInvalid,
            "rejected",
        ))) else {
            panic!("interruption lost rejection evidence")
        };
        assert_eq!(error.code, ErrorCode::Canceled);
        assert_eq!(error.causes[0].code, ErrorCode::ManifestInvalid);
        let root = crate::test_support::TestDir::new("launch-cancel");
        let run_dir = root.join("runs/session");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&run_dir)
            .unwrap();
        let acknowledgement = || Acknowledgement {
            run_id: "session".into(),
            run_dir: run_dir.clone(),
            logs_dir: run_dir.join("logs"),
        };
        assert!(matches!(
            interrupt(LaunchOutcome::Established(acknowledgement())),
            LaunchOutcome::CancellationUndelivered { cause, .. }
                if cause.code == ErrorCode::LifecycleFailed
        ));

        let token = crate::cancellation::CancellationToken::new();
        let _control = crate::session_control::SessionControl::establish(
            crate::filesystem::Directory::private_anchor(&run_dir).unwrap(),
            &run_dir,
            &token,
        )
        .unwrap();
        assert!(matches!(
            interrupt(LaunchOutcome::Established(acknowledgement())),
            LaunchOutcome::CanceledAfterEstablishment(_)
        ));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !token.is_canceled() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(token.is_canceled());
    }

    #[test]
    fn closed_launcher_is_observed_abandonment_and_rejection_follows_acknowledgement_rules() {
        let (mut launcher, owner) = UnixStream::pair().unwrap();
        launcher.set_nonblocking(true).unwrap();
        owner.set_nonblocking(true).unwrap();
        let mut establishment = Establishment {
            channel: owner,
            purpose: Purpose::Daemon,
            phase: Phase::Precommit,
        };
        assert!(establishment.check_abandonment().is_ok());
        launcher.shutdown(std::net::Shutdown::Write).unwrap();
        let error = establishment.check_abandonment().unwrap_err();
        assert_eq!(error.code, ErrorCode::Canceled);

        establishment.mark_committed();
        establishment.write_reply(serde_json::to_vec(&DaemonReply::Established(
            Acknowledgement {
                run_id: "run-1".into(),
                run_dir: "/r".into(),
                logs_dir: "/r/logs".into(),
            },
        )));
        // After acknowledgement a later failure is the session's own outcome.
        establishment.reject(&RuntimeError::new(ErrorCode::TaskFailed, "later"));
        drop(establishment);
        let body = PROTOCOL
            .read(&mut launcher, Instant::now() + REPLY_TIMEOUT)
            .unwrap();
        assert!(matches!(
            serde_json::from_slice::<DaemonReply>(&body).unwrap(),
            DaemonReply::Established(_)
        ));
    }

    #[test]
    fn run_identities_are_single_safe_components() {
        let generated = crate::registry::session::new_run_id().unwrap();
        for valid in ["run-abc", generated.as_str()] {
            assert!(valid_run_id(valid), "{valid:?}");
        }
        for invalid in ["", "../x", "a/b", "run 1", "run\0"] {
            assert!(!valid_run_id(invalid), "{invalid:?}");
        }
        assert!(valid_run_id(&"a".repeat(128)));
        for invalid in [
            "a".repeat(129),
            "run_1".into(),
            "run.1".into(),
            "run-é".into(),
        ] {
            assert!(!valid_run_id(&invalid));
        }
    }
}
