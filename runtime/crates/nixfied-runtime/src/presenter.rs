//! Command-owned, read-only presentation of a session's retained evidence.
//!
//! The session owner never writes application bytes to caller streams while it
//! has duties. It registers redacted per-source files and a diagnostic source;
//! this auxiliary process tails them by run identity and offset, discovers new
//! sources in short read-only transactions, and drains stable files once the
//! owner publishes the output seal. A blocked reader can stall only this
//! process. Its private socket carries one bounded initialization, the owner's
//! finish or cancel byte, and one bounded delivery report; never log bytes.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::output::{
    DIAGNOSTIC_SOURCE, OutputStream, ProjectionIssue, ProjectionOperation, SourcePresentation,
};
use crate::registry::{RegistryIdentity, RegistryReader};

pub const COMMAND: &str = "__presenter";
const MAGIC: &[u8; 4] = b"NXP1";
const MAX_FRAME: usize = 64 * 1024;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const DISCOVERY_INTERVAL: Duration = Duration::from_millis(50);
const CHUNK: usize = 64 * 1024;
/// Per-source bytes read per discovery pass, so one busy source cannot starve others.
const SOURCE_BUDGET: usize = 1024 * 1024;
/// A human line longer than this is emitted as a labeled fragment.
const MAX_LINE: usize = 8 * 1024;
const WRITER_QUEUE: usize = 64;
const FAILURE_EXIT: i32 = 125;
const FINISH: u8 = b'F';
const CANCEL: u8 = b'C';

/// Which evidence reaches which caller stream. `json` output needs no presenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresentationMode {
    /// Labeled output of executed tasks, preparation, and services on stderr.
    Human,
    /// Exact bytes of the directly selected task on the matching streams.
    TaskOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresenterInit {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub registry_path: PathBuf,
    pub project_id: String,
    pub environment: String,
    pub slot: i64,
    pub runtime_abi: String,
    pub toolchain_id: String,
    pub mode: PresentationMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PresenterReport {
    sealed: bool,
    issues: Vec<ProjectionIssue>,
}

/// The command-local result of presentation. It never rewrites session state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// Every byte of a sealed source set reached the caller streams.
    Delivered,
    /// A caller stream failed; the issues describe which and how.
    Failed(Vec<ProjectionIssue>),
    /// A termination signal ended the drain before completion.
    Canceled,
    /// The presentation helper failed or could not be observed.
    HelperFailed,
    /// No trustworthy seal was observed, so completeness is unknown.
    Unknown,
}

/// The sole owner and reaper of the presentation child.
pub struct CommandPresenter {
    child: Option<Child>,
    channel: UnixStream,
}

impl CommandPresenter {
    /// Launch the helper while the slot is held: it inherits only caller
    /// streams and its private socket, never locks, secrets, or FIFOs.
    pub fn spawn(
        authority: &crate::state::ownership::SlotGuard,
        launcher: &Path,
        init: &PresenterInit,
    ) -> RuntimeResult<Self> {
        use std::os::unix::process::CommandExt;
        let failure = |message: &str| RuntimeError::new(ErrorCode::LifecycleFailed, message);
        let body = serde_json::to_vec(init)
            .map_err(|_| failure("cannot encode presenter initialization"))?;
        if body.len() > MAX_FRAME {
            return Err(failure("presenter initialization exceeds its limit"));
        }
        let (mut channel, child_channel) = crate::launch::startup_pair()
            .map_err(|_| failure("cannot create presenter channel"))?;
        let inherited = child_channel.as_raw_fd();
        let owner_end = channel.as_raw_fd();
        let mut command = Command::new(launcher);
        command
            .arg(COMMAND)
            .arg(inherited.to_string())
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        // SAFETY: only async-signal-safe descriptor syscalls run after fork.
        unsafe {
            command.pre_exec(move || {
                if libc::close(owner_end) != 0 || libc::fcntl(inherited, libc::F_SETFD, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = authority
            .spawn(command)
            .map_err(|_| failure("cannot spawn presenter"))?;
        drop(child_channel);
        let mut presenter = Self {
            child: Some(child),
            channel: channel
                .try_clone()
                .map_err(|_| failure("cannot own presenter channel"))?,
        };
        let mut frame = Vec::with_capacity(8 + body.len());
        frame.extend(MAGIC);
        frame.extend((body.len() as u32).to_be_bytes());
        frame.extend(body);
        if write_all_until(&mut channel, &frame, Instant::now() + STARTUP_TIMEOUT).is_err() {
            presenter.kill_and_reap();
            return Err(failure("cannot initialize presenter"));
        }
        Ok(presenter)
    }

    /// After slot release: let the helper drain stable evidence with no
    /// default deadline. A termination signal that arrives during the drain
    /// ends it; one that already canceled the session does not truncate the
    /// final output the session retained.
    pub fn finish(mut self) -> DeliveryOutcome {
        let signals = crate::cancellation::signal_count();
        let _ = write_all_until(
            &mut self.channel,
            &[FINISH],
            Instant::now() + Duration::from_secs(1),
        );
        let status = loop {
            let Some(child) = self.child.as_mut() else {
                return DeliveryOutcome::Unknown;
            };
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if crate::cancellation::signal_count() > signals => {
                    let _ = self.channel.write(&[CANCEL]);
                    self.kill_and_reap();
                    return DeliveryOutcome::Canceled;
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => {
                    self.kill_and_reap();
                    return DeliveryOutcome::HelperFailed;
                }
            }
        };
        self.child = None;
        if !status.success() {
            return DeliveryOutcome::HelperFailed;
        }
        match read_frame(&mut self.channel, Instant::now() + Duration::from_secs(1))
            .ok()
            .and_then(|body| serde_json::from_slice::<PresenterReport>(&body).ok())
        {
            Some(report) if !report.issues.is_empty() => DeliveryOutcome::Failed(report.issues),
            Some(report) if report.sealed => DeliveryOutcome::Delivered,
            Some(_) | None => DeliveryOutcome::Unknown,
        }
    }

    fn kill_and_reap(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for CommandPresenter {
    fn drop(&mut self) {
        self.kill_and_reap();
    }
}

impl DeliveryOutcome {
    /// Combine with the session result without rewriting session outcomes:
    /// a failed or unconfirmed delivery fails an otherwise successful command.
    pub fn into_error(self, session_canceled: bool) -> Option<RuntimeError> {
        match self {
            Self::Delivered => None,
            Self::Failed(issues) => Some(crate::output::projection_error(
                "live output delivery failed",
                issues,
            )),
            Self::Canceled | Self::HelperFailed if session_canceled => None,
            Self::Canceled => Some(crate::output::projection_error(
                "live output delivery was canceled",
                vec![ProjectionIssue::named(
                    OutputStream::Stdout,
                    "canceled",
                    Path::new("<presenter>"),
                )],
            )),
            Self::HelperFailed => Some(crate::output::projection_error(
                "the live output presenter failed",
                vec![ProjectionIssue::named(
                    OutputStream::Stdout,
                    "presenter-failed",
                    Path::new("<presenter>"),
                )],
            )),
            Self::Unknown if session_canceled => None,
            Self::Unknown => Some(crate::output::projection_error(
                "live output completeness could not be confirmed",
                vec![ProjectionIssue::named(
                    OutputStream::Stdout,
                    "unsealed",
                    Path::new("<presenter>"),
                )],
            )),
        }
    }
}

/// Return None for ordinary runtime commands. Runs before signal ownership.
pub fn dispatch(args: &[OsString]) -> Option<i32> {
    if args.first().is_none_or(|arg| arg != COMMAND) {
        return None;
    }
    let [_, descriptor] = args else {
        return Some(FAILURE_EXIT);
    };
    let Some(fd) = descriptor
        .to_str()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|fd| *fd >= 3)
    else {
        return Some(FAILURE_EXIT);
    };
    let mut kind: libc::c_int = 0;
    let mut kind_length = std::mem::size_of_val(&kind) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&raw mut kind).cast(),
            &mut kind_length,
        )
    } != 0
        || kind != libc::SOCK_STREAM
    {
        return Some(FAILURE_EXIT);
    }
    // Delivery reports EPIPE as a typed failure instead of dying. Termination
    // signals keep their default action: Ctrl-C ends presentation at once.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    // SAFETY: this internal process takes sole ownership of its inherited socket.
    let mut channel = unsafe { UnixStream::from_raw_fd(fd) };
    if channel.set_nonblocking(true).is_err() {
        return Some(FAILURE_EXIT);
    }
    let Some(init) = read_frame(&mut channel, Instant::now() + STARTUP_TIMEOUT)
        .ok()
        .and_then(|body| serde_json::from_slice::<PresenterInit>(&body).ok())
    else {
        return Some(FAILURE_EXIT);
    };
    Some(present(init, channel))
}

fn present(init: PresenterInit, channel: UnixStream) -> i32 {
    let finishing = Arc::new(AtomicBool::new(false));
    let Ok(control) = channel.try_clone() else {
        return FAILURE_EXIT;
    };
    {
        let finishing = Arc::clone(&finishing);
        // Parent loss or cancellation ends presentation without joining
        // writers that may be blocked on a stalled reader.
        let spawned = thread::Builder::new()
            .name("nixfied-presenter-control".into())
            .spawn(move || watch_control(control, &finishing));
        if spawned.is_err() {
            return FAILURE_EXIT;
        }
    }
    let stdout = Writer::spawn(OutputStream::Stdout);
    let stderr = Writer::spawn(OutputStream::Stderr);
    let (Ok(stdout), Ok(stderr)) = (stdout, stderr) else {
        return FAILURE_EXIT;
    };
    let identity = RegistryIdentity {
        project_id: init.project_id.clone(),
        environment: init.environment.clone(),
        slot: init.slot,
        runtime_abi: init.runtime_abi.clone(),
        toolchain_id: init.toolchain_id.clone(),
    };
    let mut tails: BTreeMap<String, Tail> = BTreeMap::new();
    let sealed = loop {
        let finishing_now = finishing.load(Ordering::SeqCst);
        // Discovery happens before draining: a seal observed here means every
        // writer closed, so the drain below reaches the final bytes.
        let snapshot = discover(&init, &identity);
        if let Ok(snapshot) = &snapshot {
            for source in &snapshot.sources {
                tails
                    .entry(source.relative.clone())
                    .or_insert_with(|| Tail::new(&init.run_dir, source));
            }
        }
        for tail in tails.values_mut() {
            tail.pump(&stdout, &stderr);
        }
        let sealed = snapshot.as_ref().is_ok_and(|snapshot| snapshot.sealed);
        if sealed || finishing_now {
            // Drain every stable file to its end, however large the backlog;
            // a slow healthy reader is never truncated.
            for tail in tails.values_mut() {
                tail.require_open();
            }
            loop {
                let moved = tails
                    .values_mut()
                    .map(|tail| tail.pump(&stdout, &stderr))
                    .sum::<usize>();
                let waiting = tails.values().any(|tail| tail.blocked.is_some());
                if moved == 0 && !waiting {
                    break;
                }
                if moved == 0 {
                    thread::sleep(Duration::from_millis(5));
                }
            }
            for tail in tails.values_mut() {
                tail.flush_partial(&stdout, &stderr);
            }
            while tails.values().any(|tail| tail.blocked.is_some()) {
                for tail in tails.values_mut() {
                    tail.retry(&stdout, &stderr);
                }
                thread::sleep(Duration::from_millis(5));
            }
            break sealed;
        }
        thread::sleep(DISCOVERY_INTERVAL);
    };
    let mut issues: Vec<ProjectionIssue> = tails
        .values_mut()
        .filter_map(|tail| tail.issue.take())
        .collect();
    issues.extend(stdout.finish());
    issues.extend(stderr.finish());
    let Ok(body) = serde_json::to_vec(&PresenterReport { sealed, issues }) else {
        return FAILURE_EXIT;
    };
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend(MAGIC);
    frame.extend((body.len() as u32).to_be_bytes());
    frame.extend(body);
    let mut channel = channel;
    if write_all_until(
        &mut channel,
        &frame,
        Instant::now() + Duration::from_secs(1),
    )
    .is_err()
    {
        return FAILURE_EXIT;
    }
    0
}

fn watch_control(mut control: UnixStream, finishing: &AtomicBool) {
    let _ = control.set_nonblocking(false);
    let mut byte = [0_u8; 1];
    loop {
        match control.read(&mut byte) {
            Ok(1) if byte[0] == FINISH => finishing.store(true, Ordering::SeqCst),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // Cancellation, owner loss (EOF), or a malformed control byte.
            _ => std::process::exit(FAILURE_EXIT),
        }
    }
}

struct DiscoveredSource {
    relative: String,
    route: Route,
}

#[derive(Clone)]
enum Route {
    /// Raw bytes to one caller stream.
    Raw(OutputStream),
    /// Labeled lines on stderr.
    Labeled(Vec<u8>),
}

struct Snapshot {
    sources: Vec<DiscoveredSource>,
    sealed: bool,
}

/// One short read-only transaction per pass; never a writer and never the
/// latest slot occupant, only this run's committed sources.
fn discover(init: &PresenterInit, identity: &RegistryIdentity) -> RuntimeResult<Snapshot> {
    let reader =
        RegistryReader::open_existing(&init.registry_path, identity)?.ok_or_else(|| {
            RuntimeError::new(ErrorCode::RegistryCorrupt, "session registry is missing")
        })?;
    let connection = reader.connection();
    let sql =
        |error: rusqlite::Error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string());
    let (diagnostic, output): (String, String) = connection
        .query_row(
            "SELECT diagnostic_path, output FROM runs WHERE run_id = ?1",
            [&init.run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sql)?;
    let mut sources = vec![DiscoveredSource {
        relative: validated(&diagnostic)?,
        route: Route::Raw(OutputStream::Stderr),
    }];
    let mut statement = connection
        .prepare(
            "SELECT source_label, presentation, stdout_path, stderr_path FROM processes
             WHERE run_id = ?1 ORDER BY rowid",
        )
        .map_err(sql)?;
    let rows = statement
        .query_map([&init.run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    for (label, presentation, stdout, stderr) in rows {
        let presentation = SourcePresentation::parse(&presentation).ok_or_else(|| {
            RuntimeError::new(ErrorCode::RegistryCorrupt, "unknown source presentation")
        })?;
        let routes = match (init.mode, presentation) {
            (_, SourcePresentation::Hidden) => None,
            (PresentationMode::TaskOutput, SourcePresentation::Selected) => Some((
                Route::Raw(OutputStream::Stdout),
                Route::Raw(OutputStream::Stderr),
            )),
            (PresentationMode::TaskOutput, SourcePresentation::Shown) => None,
            (PresentationMode::Human, _) => Some((
                Route::Labeled(format!("[{label}] ").into_bytes()),
                Route::Labeled(format!("[{label}:err] ").into_bytes()),
            )),
        };
        if let Some((stdout_route, stderr_route)) = routes {
            sources.push(DiscoveredSource {
                relative: validated(&stdout)?,
                route: stdout_route,
            });
            sources.push(DiscoveredSource {
                relative: validated(&stderr)?,
                route: stderr_route,
            });
        }
    }
    Ok(Snapshot {
        sources,
        sealed: output == "sealed",
    })
}

/// Machine locators are the run's diagnostic source or one `logs/<file>`.
fn validated(relative: &str) -> RuntimeResult<String> {
    let path = Path::new(relative);
    let components: Vec<_> = path.components().collect();
    let valid = relative == DIAGNOSTIC_SOURCE
        || matches!(
            components.as_slice(),
            [Component::Normal(directory), Component::Normal(_)] if *directory == "logs"
        );
    if !valid {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "source path escapes the run evidence directory",
        ));
    }
    Ok(relative.to_owned())
}

struct Tail {
    path: PathBuf,
    file: Option<File>,
    route: Route,
    partial: Vec<u8>,
    /// Rendered bytes a full queue refused. While present this source reads
    /// nothing more, so a stalled stream holds back only its own sources.
    blocked: Option<(OutputStream, Vec<u8>)>,
    /// A failure reading this source's retained evidence; completeness of
    /// its delivery is then not claimed.
    issue: Option<ProjectionIssue>,
}

impl Tail {
    fn new(run_dir: &Path, source: &DiscoveredSource) -> Self {
        Self {
            path: run_dir.join(&source.relative),
            file: None,
            route: source.route.clone(),
            partial: Vec::new(),
            blocked: None,
            issue: None,
        }
    }

    fn stream(&self) -> OutputStream {
        match self.route {
            Route::Raw(stream) => stream,
            Route::Labeled(_) => OutputStream::Stderr,
        }
    }

    /// At the final drain a registered source that still cannot be opened is
    /// a delivery failure, not an empty source.
    fn require_open(&mut self) {
        if self.file.is_none() && self.issue.is_none() {
            match std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&self.path)
            {
                Ok(file) => self.file = Some(file),
                Err(error) => {
                    self.issue = Some(ProjectionIssue::io(
                        self.stream(),
                        ProjectionOperation::Open,
                        &self.path,
                        &error,
                        0,
                    ));
                }
            }
        }
    }

    /// Temporary EOF while capture is active is not completion; the offset
    /// simply waits for more bytes. Returns the bytes moved in this pass.
    fn pump(&mut self, stdout: &Writer, stderr: &Writer) -> usize {
        let mut moved = self.retry(stdout, stderr);
        if self.blocked.is_some() || self.issue.is_some() {
            return moved;
        }
        if self.file.is_none() {
            self.file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&self.path)
                .ok();
        }
        let Some(mut file) = self.file.take() else {
            return moved;
        };
        let mut budget = SOURCE_BUDGET;
        let mut buffer = vec![0_u8; CHUNK];
        while budget > 0 && self.blocked.is_none() {
            let read = match file.read(&mut buffer[..CHUNK.min(budget)]) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.issue = Some(ProjectionIssue::io(
                        self.stream(),
                        ProjectionOperation::Read,
                        &self.path,
                        &error,
                        0,
                    ));
                    break;
                }
            };
            budget -= read;
            moved += read;
            self.deliver(&buffer[..read], stdout, stderr);
        }
        self.file = Some(file);
        moved
    }

    fn retry(&mut self, stdout: &Writer, stderr: &Writer) -> usize {
        match self.blocked.take() {
            Some((stream, bytes)) => {
                let length = bytes.len();
                self.offer(stream, bytes, stdout, stderr);
                if self.blocked.is_some() { 0 } else { length }
            }
            None => 0,
        }
    }

    fn offer(&mut self, stream: OutputStream, bytes: Vec<u8>, stdout: &Writer, stderr: &Writer) {
        let writer = match stream {
            OutputStream::Stdout => stdout,
            OutputStream::Stderr => stderr,
        };
        if let Err(refused) = writer.try_send(bytes) {
            self.blocked = Some((stream, refused));
        }
    }

    fn deliver(&mut self, bytes: &[u8], stdout: &Writer, stderr: &Writer) {
        match &self.route {
            Route::Raw(stream) => {
                let stream = *stream;
                self.offer(stream, bytes.to_vec(), stdout, stderr);
            }
            Route::Labeled(label) => {
                let label = label.clone();
                self.partial.extend_from_slice(bytes);
                let mut rendered = Vec::new();
                while let Some(end) = self.partial.iter().position(|byte| *byte == b'\n') {
                    rendered.extend_from_slice(&label);
                    rendered.extend(self.partial.drain(..=end));
                }
                // A source without a newline cannot consume unbounded memory.
                while self.partial.len() > MAX_LINE {
                    rendered.extend_from_slice(&label);
                    rendered.extend(self.partial.drain(..MAX_LINE));
                    rendered.push(b'\n');
                }
                if !rendered.is_empty() {
                    self.offer(OutputStream::Stderr, rendered, stdout, stderr);
                }
            }
        }
    }

    fn flush_partial(&mut self, stdout: &Writer, stderr: &Writer) {
        if let Route::Labeled(label) = &self.route
            && !self.partial.is_empty()
        {
            let mut rendered = label.clone();
            rendered.append(&mut self.partial);
            rendered.push(b'\n');
            match self.blocked.as_mut() {
                // Keep order: append behind bytes the queue already refused.
                Some((_, pending)) => pending.extend(rendered),
                None => self.offer(OutputStream::Stderr, rendered, stdout, stderr),
            }
        }
    }
}

/// One bounded queue and worker per caller stream, so a blocked stdout reader
/// cannot stop stderr delivery. A failed stream records its issue and drops
/// the rest of its bytes.
struct Writer {
    sender: Option<SyncSender<Vec<u8>>>,
    worker: Option<JoinHandle<Vec<ProjectionIssue>>>,
}

impl Writer {
    fn spawn(stream: OutputStream) -> io::Result<Self> {
        let (sender, receiver) = sync_channel::<Vec<u8>>(WRITER_QUEUE);
        let worker = thread::Builder::new()
            .name(format!("nixfied-presenter-{stream:?}").to_lowercase())
            .spawn(move || deliver(stream, receiver))?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    /// Never blocks: a full queue hands the bytes back. A disconnected
    /// queue (a failed stream) accepts and drops them.
    fn try_send(&self, bytes: Vec<u8>) -> Result<(), Vec<u8>> {
        match &self.sender {
            Some(sender) => match sender.try_send(bytes) {
                Err(std::sync::mpsc::TrySendError::Full(bytes)) => Err(bytes),
                Ok(()) | Err(std::sync::mpsc::TrySendError::Disconnected(_)) => Ok(()),
            },
            None => Ok(()),
        }
    }

    fn finish(mut self) -> Vec<ProjectionIssue> {
        drop(self.sender.take());
        let stream_path = Path::new("<presenter>");
        match self.worker.take().map(JoinHandle::join) {
            Some(Ok(issues)) => issues,
            _ => vec![ProjectionIssue::named(
                OutputStream::Stdout,
                "worker-panic",
                stream_path,
            )],
        }
    }
}

fn deliver(stream: OutputStream, receiver: Receiver<Vec<u8>>) -> Vec<ProjectionIssue> {
    let name = match stream {
        OutputStream::Stdout => "<stdout>",
        OutputStream::Stderr => "<stderr>",
    };
    let mut sink: Box<dyn Write> = match stream {
        OutputStream::Stdout => Box::new(io::stdout().lock()),
        OutputStream::Stderr => Box::new(io::stderr().lock()),
    };
    let mut written = 0_u64;
    let mut issue = None;
    for bytes in receiver {
        if issue.is_some() {
            continue;
        }
        let result = sink.write_all(&bytes).and_then(|()| sink.flush());
        match result {
            Ok(()) => written = written.saturating_add(bytes.len() as u64),
            Err(error) => {
                issue = Some(ProjectionIssue::io(
                    stream,
                    ProjectionOperation::Write,
                    Path::new(name),
                    &error,
                    written,
                ));
            }
        }
    }
    issue.into_iter().collect()
}

fn write_all_until(
    channel: &mut UnixStream,
    mut bytes: &[u8],
    deadline: Instant,
) -> io::Result<()> {
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

fn read_frame(channel: &mut UnixStream, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut header = [0_u8; 8];
    read_exact_until(channel, &mut header, deadline)?;
    if &header[..4] != MAGIC {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let size = u32::from_be_bytes(header[4..].try_into().expect("four bytes")) as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0_u8; size];
    read_exact_until(channel, &mut body, deadline)?;
    Ok(body)
}

fn read_exact_until(
    channel: &mut UnixStream,
    mut bytes: &mut [u8],
    deadline: Instant,
) -> io::Result<()> {
    while !bytes.is_empty() {
        match channel.read(bytes) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => bytes = &mut bytes[read..],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_locators_cannot_escape_the_run_directory() {
        for valid in ["diagnostics.log", "logs/task.0.stdout.log"] {
            assert_eq!(validated(valid).unwrap(), valid);
        }
        for invalid in [
            "",
            "/etc/passwd",
            "../registry.sqlite3",
            "logs/../../x",
            "logs/a/b",
            "other/x",
            "logs",
        ] {
            assert!(validated(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn labeled_rendering_bounds_fragments_and_preserves_line_order() {
        let (stdout, stdout_rx) = test_writer();
        let (stderr, stderr_rx) = test_writer();
        let mut tail = Tail {
            path: PathBuf::new(),
            file: None,
            route: Route::Labeled(b"[api] ".to_vec()),
            partial: Vec::new(),
            blocked: None,
            issue: None,
        };
        tail.deliver(b"one\ntw", &stdout, &stderr);
        tail.deliver(b"o\n", &stdout, &stderr);
        tail.deliver(&vec![b'x'; MAX_LINE + 3], &stdout, &stderr);
        tail.flush_partial(&stdout, &stderr);
        drop((stdout, stderr));
        assert!(stdout_rx.try_iter().next().is_none());
        let rendered: Vec<u8> = stderr_rx.try_iter().flatten().collect();
        let mut expected = b"[api] one\n[api] two\n[api] ".to_vec();
        expected.extend(vec![b'x'; MAX_LINE]);
        expected.extend(b"\n[api] xxx\n");
        assert_eq!(rendered, expected);
    }

    fn test_writer() -> (Writer, Receiver<Vec<u8>>) {
        let (sender, receiver) = sync_channel(64);
        (
            Writer {
                sender: Some(sender),
                worker: None,
            },
            receiver,
        )
    }
}
