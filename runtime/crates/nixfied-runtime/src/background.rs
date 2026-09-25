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
use std::io::{self, Read};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

pub const COMMAND: &str = "__session-owner";
const PROTOCOL: crate::channel::Protocol = crate::channel::Protocol {
    magic: *b"NXD1",
    max: 1024 * 1024,
};
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const REPLY_TIMEOUT: Duration = Duration::from_secs(1);

/// The launcher's validated request: forwarded run arguments and the
/// immutable run identity it allocated before spawning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub run_id: String,
    pub args: Vec<String>,
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
enum Reply {
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

/// Request cancellation of exactly the acknowledged session and report
/// whether the request reached its owner.
fn cancel_established(acknowledgement: Acknowledgement) -> LaunchOutcome {
    use crate::session_control::{CancellationDelivery, request_cancellation};
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
    use std::os::unix::process::CommandExt;
    let failure = |message: &str| RuntimeError::new(ErrorCode::LifecycleFailed, message);
    let body = serde_json::to_vec(request).map_err(|_| failure("cannot encode launch request"))?;
    let frame = PROTOCOL
        .encode(&body)
        .map_err(|_| failure("background launch request exceeds its limit"))?;
    let (mut channel, child_channel) =
        crate::channel::pair().map_err(|_| failure("cannot create background launch channel"))?;
    let mut command = Command::new(runtime);
    command.arg(COMMAND);
    crate::channel::inherit(&mut command, &child_channel, &channel);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe. The owner leads a new session so
    // it holds no controlling terminal.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut owner = crate::spawn::command(&mut command)
        .map_err(|_| failure("cannot spawn background session owner"))?;
    drop(child_channel);
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
                break match serde_json::from_slice::<Reply>(&body) {
                    Ok(Reply::Established(acknowledgement)) if interrupted => {
                        cancel_established(acknowledgement)
                    }
                    Ok(Reply::Established(acknowledgement)) => {
                        LaunchOutcome::Established(acknowledgement)
                    }
                    Ok(Reply::Rejected(rejection)) => LaunchOutcome::Rejected(
                        RuntimeError::new(rejection.code, rejection.message)
                            .with_details(rejection.details),
                    ),
                    Err(_) => LaunchOutcome::Uncertain,
                };
            }
            Ok(None) if Instant::now() < deadline => {
                let _ = crate::channel::wait(&channel, libc::POLLIN, deadline);
            }
            Ok(None) | Err(_) if interrupted => break LaunchOutcome::Interrupted,
            Ok(None) | Err(_) => break LaunchOutcome::Uncertain,
        }
    };
    // An interruption that arrives with the acknowledgement still cancels
    // exactly the acknowledged session.
    let outcome = match outcome {
        LaunchOutcome::Established(acknowledgement) if crate::cancellation::signal_received() => {
            cancel_established(acknowledgement)
        }
        outcome => outcome,
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
    Ok(outcome)
}

/// The owner's end of the channel between request and establishment.
pub struct Establishment {
    channel: UnixStream,
    acknowledged: bool,
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
    if !valid_run_id(&request.run_id) {
        return failure;
    }
    Some(Ok((
        request,
        Establishment {
            channel,
            acknowledged: false,
        },
    )))
}

impl Establishment {
    /// Observed startup abandonment: the launcher closed its end before the
    /// owner committed. Data after the request is also a protocol violation.
    pub fn check_abandonment(&mut self) -> RuntimeResult<()> {
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
    pub fn acknowledge(&mut self, acknowledgement: Acknowledgement) {
        self.acknowledged = true;
        if let Ok(body) = serde_json::to_vec(&Reply::Established(acknowledgement)) {
            let _ = PROTOCOL.write(&mut self.channel, &body, Instant::now() + REPLY_TIMEOUT);
        }
    }

    /// Report a pre-establishment failure; after acknowledgement, failures
    /// belong to the session's own recorded outcome.
    pub fn reject(&mut self, error: &RuntimeError) {
        if self.acknowledged {
            return;
        }
        let rejection = Reply::Rejected(Rejection {
            code: error.code,
            message: error.message.clone(),
            details: error.details.clone(),
        });
        if let Ok(body) = serde_json::to_vec(&rejection) {
            let _ = PROTOCOL.write(&mut self.channel, &body, Instant::now() + REPLY_TIMEOUT);
        }
    }
}

/// Launcher-allocated identities are single path components.
fn valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= 128
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_cancellation_reports_whether_it_reached_the_owner() {
        use std::os::unix::fs::DirBuilderExt;
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
            cancel_established(acknowledgement()),
            LaunchOutcome::CancellationUndelivered { cause, .. }
                if cause.code == ErrorCode::LifecycleFailed
        ));

        let token = crate::cancellation::CancellationToken::new();
        let control = crate::session_control::SessionControl::establish(
            crate::filesystem::Directory::private_anchor(&run_dir).unwrap(),
            &run_dir,
            &token,
        )
        .unwrap();
        assert!(matches!(
            cancel_established(acknowledgement()),
            LaunchOutcome::CanceledAfterEstablishment(_)
        ));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !token.is_canceled() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(token.is_canceled());
        drop(control);
    }

    #[test]
    fn closed_launcher_is_observed_abandonment_and_rejection_follows_acknowledgement_rules() {
        let (launcher, owner) = UnixStream::pair().unwrap();
        owner.set_nonblocking(true).unwrap();
        let mut establishment = Establishment {
            channel: owner,
            acknowledged: false,
        };
        assert!(establishment.check_abandonment().is_ok());
        launcher.shutdown(std::net::Shutdown::Write).unwrap();
        let error = establishment.check_abandonment().unwrap_err();
        assert_eq!(error.code, ErrorCode::Canceled);

        let (mut launcher, owner) = UnixStream::pair().unwrap();
        owner.set_nonblocking(true).unwrap();
        launcher.set_nonblocking(true).unwrap();
        let mut establishment = Establishment {
            channel: owner,
            acknowledged: false,
        };
        establishment.acknowledge(Acknowledgement {
            run_id: "run-1".into(),
            run_dir: "/r".into(),
            logs_dir: "/r/logs".into(),
        });
        // After acknowledgement a later failure is the session's own outcome.
        establishment.reject(&RuntimeError::new(ErrorCode::TaskFailed, "later"));
        drop(establishment);
        let body = PROTOCOL
            .read(&mut launcher, Instant::now() + REPLY_TIMEOUT)
            .unwrap();
        assert!(matches!(
            serde_json::from_slice::<Reply>(&body).unwrap(),
            Reply::Established(_)
        ));
    }

    #[test]
    fn run_identities_are_single_safe_components() {
        for valid in ["run-12-34", "run-abc"] {
            assert!(valid_run_id(valid));
        }
        for invalid in ["", "../x", "a/b", "run 1", "run\0"] {
            assert!(!valid_run_id(invalid), "{invalid:?}");
        }
    }
}
