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
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

pub const COMMAND: &str = "__session-owner";
const MAGIC: &[u8; 4] = b"NXD1";
const MAX_FRAME: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const REPLY_TIMEOUT: Duration = Duration::from_secs(1);
const FAILURE_EXIT: i32 = 125;

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
}

/// After an interruption the launcher waits this long for a conclusive reply.
const INTERRUPT_GRACE: Duration = Duration::from_secs(10);

/// Spawn the owner, send the request, and wait for one bounded reply. The
/// launcher keeps its end open while waiting: closing it means abandonment.
pub fn launch(runtime: &Path, request: &Request, wait: Duration) -> RuntimeResult<LaunchOutcome> {
    use std::os::unix::process::CommandExt;
    let failure = |message: &str| RuntimeError::new(ErrorCode::LifecycleFailed, message);
    let body = serde_json::to_vec(request).map_err(|_| failure("cannot encode launch request"))?;
    if body.len() > MAX_FRAME {
        return Err(failure("background launch request exceeds its limit"));
    }
    let (mut channel, child_channel) = crate::launch::startup_pair()
        .map_err(|_| failure("cannot create background launch channel"))?;
    let inherited = child_channel.as_raw_fd();
    let launcher_end = channel.as_raw_fd();
    let mut command = Command::new(runtime);
    command
        .arg(COMMAND)
        .arg(inherited.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: only async-signal-safe syscalls run after fork. The owner leads
    // a new session so it holds no controlling terminal.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0
                || libc::close(launcher_end) != 0
                || libc::fcntl(inherited, libc::F_SETFD, 0) != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut owner = crate::spawn::command(&mut command)
        .map_err(|_| failure("cannot spawn background session owner"))?;
    drop(child_channel);
    if write_frame(&mut channel, &body, Instant::now() + REQUEST_TIMEOUT).is_err() {
        let _ = owner.kill();
        let _ = owner.wait();
        return Err(failure("cannot deliver the background launch request"));
    }
    let mut deadline = Instant::now() + wait;
    let mut received = Vec::new();
    let mut interrupted = false;
    let outcome = loop {
        if !interrupted && crate::cancellation::signal_received() {
            // Half-closing is the abandonment signal; the reply side stays
            // open so a won race is still observed and canceled precisely.
            interrupted = true;
            let _ = channel.shutdown(std::net::Shutdown::Write);
            deadline = Instant::now() + INTERRUPT_GRACE;
        }
        match read_frame_step(&mut channel, &mut received) {
            Ok(Some(body)) => {
                break match serde_json::from_slice::<Reply>(&body) {
                    Ok(Reply::Established(acknowledgement)) if interrupted => {
                        let _ =
                            crate::session_control::request_cancellation(&acknowledgement.run_dir);
                        LaunchOutcome::CanceledAfterEstablishment(acknowledgement)
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
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) | Err(_) if interrupted => break LaunchOutcome::Interrupted,
            Ok(None) | Err(_) => break LaunchOutcome::Uncertain,
        }
    };
    // An interruption that arrives with the acknowledgement still cancels
    // exactly the acknowledged session.
    let outcome = match outcome {
        LaunchOutcome::Established(acknowledgement) if crate::cancellation::signal_received() => {
            let _ = crate::session_control::request_cancellation(&acknowledgement.run_dir);
            LaunchOutcome::CanceledAfterEstablishment(acknowledgement)
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
    if args.first().is_none_or(|arg| arg != COMMAND) {
        return None;
    }
    let [_, descriptor] = args else {
        return Some(Err(FAILURE_EXIT));
    };
    let Some(fd) = descriptor
        .to_str()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|fd| *fd >= 3)
    else {
        return Some(Err(FAILURE_EXIT));
    };
    let mut kind: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&raw mut kind).cast(),
            &mut length,
        )
    } != 0
        || kind != libc::SOCK_STREAM
    {
        return Some(Err(FAILURE_EXIT));
    }
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    // SAFETY: this internal process takes sole ownership of its inherited socket.
    let mut channel = unsafe { UnixStream::from_raw_fd(fd) };
    if channel.set_nonblocking(true).is_err() {
        return Some(Err(FAILURE_EXIT));
    }
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut received = Vec::new();
    let body = loop {
        match read_frame_step(&mut channel, &mut received) {
            Ok(Some(body)) => break body,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => return Some(Err(FAILURE_EXIT)),
        }
    };
    let Ok(request) = serde_json::from_slice::<Request>(&body) else {
        return Some(Err(FAILURE_EXIT));
    };
    if !valid_run_id(&request.run_id) {
        return Some(Err(FAILURE_EXIT));
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
            let _ = write_frame(&mut self.channel, &body, Instant::now() + REPLY_TIMEOUT);
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
            let _ = write_frame(&mut self.channel, &body, Instant::now() + REPLY_TIMEOUT);
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

fn write_frame(channel: &mut UnixStream, body: &[u8], deadline: Instant) -> io::Result<()> {
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend(MAGIC);
    frame.extend((body.len() as u32).to_be_bytes());
    frame.extend(body);
    let mut bytes = frame.as_slice();
    while !bytes.is_empty() {
        match channel.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Accumulate one frame without blocking. `Ok(None)` means incomplete so far;
/// EOF before a complete frame, a bad header, or trailing bytes are errors.
fn read_frame_step(
    channel: &mut UnixStream,
    received: &mut Vec<u8>,
) -> io::Result<Option<Vec<u8>>> {
    let mut chunk = [0_u8; 8192];
    let mut closed = false;
    loop {
        match channel.read(&mut chunk) {
            Ok(0) => {
                closed = true;
                break;
            }
            Ok(read) => {
                received.extend_from_slice(&chunk[..read]);
                if received.len() > MAX_FRAME + 8 {
                    return Err(io::ErrorKind::InvalidData.into());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error),
        }
    }
    let incomplete = || -> io::Result<Option<Vec<u8>>> {
        if closed {
            Err(io::ErrorKind::UnexpectedEof.into())
        } else {
            Ok(None)
        }
    };
    if received.len() < 8 {
        return incomplete();
    }
    if &received[..4] != MAGIC {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let size = u32::from_be_bytes(received[4..8].try_into().expect("four bytes")) as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    match received.len().cmp(&(8 + size)) {
        std::cmp::Ordering::Less => incomplete(),
        std::cmp::Ordering::Equal => Ok(Some(received[8..].to_vec())),
        std::cmp::Ordering::Greater => Err(io::ErrorKind::InvalidData.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_reject_truncation_trailing_bytes_and_oversize() {
        let (mut left, mut right) = UnixStream::pair().unwrap();
        right.set_nonblocking(true).unwrap();
        write_frame(&mut left, b"{}", Instant::now() + REPLY_TIMEOUT).unwrap();
        let mut received = Vec::new();
        assert_eq!(
            read_frame_step(&mut right, &mut received).unwrap(),
            Some(b"{}".to_vec())
        );

        let mut received = Vec::new();
        left.write_all(b"NXD1\0\0\0\x02{").unwrap();
        assert_eq!(read_frame_step(&mut right, &mut received).unwrap(), None);
        // Shut down explicitly: a concurrently forked test child may briefly
        // hold another copy of this descriptor.
        left.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(read_frame_step(&mut right, &mut received).is_err());

        let (mut left, mut right) = UnixStream::pair().unwrap();
        right.set_nonblocking(true).unwrap();
        left.write_all(b"NXD1\0\0\0\x02{}extra").unwrap();
        assert!(read_frame_step(&mut right, &mut Vec::new()).is_err());
        left.write_all(b"").unwrap();

        let (mut left, mut right) = UnixStream::pair().unwrap();
        right.set_nonblocking(true).unwrap();
        left.write_all(b"NXD1\xff\xff\xff\xff").unwrap();
        assert!(read_frame_step(&mut right, &mut Vec::new()).is_err());
        let (mut left, mut right) = UnixStream::pair().unwrap();
        right.set_nonblocking(true).unwrap();
        left.write_all(b"XXXX\0\0\0\x02{}").unwrap();
        assert!(read_frame_step(&mut right, &mut Vec::new()).is_err());
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
        let mut received = Vec::new();
        let body = read_frame_step(&mut launcher, &mut received)
            .unwrap()
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
