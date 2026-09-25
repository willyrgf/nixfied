use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::admission::secrets::ResolvedSecrets;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

pub const REDACTION_TOKEN: &str = "[REDACTED]";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Redactor {
    patterns: Vec<Vec<u8>>,
}

impl Redactor {
    pub fn from_secrets(secrets: &ResolvedSecrets) -> Self {
        let mut patterns = secrets
            .values()
            .map(|value| value.as_bytes().to_vec())
            .collect::<Vec<_>>();
        patterns.sort_by_key(|pattern| std::cmp::Reverse(pattern.len()));
        patterns.dedup();
        Self { patterns }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn redact_text(&self, text: &str) -> String {
        if self.is_empty() {
            return text.to_string();
        }
        String::from_utf8_lossy(&self.redact_bytes(text.as_bytes())).into_owned()
    }

    pub fn redact_json_str(&self, json: &str) -> RuntimeResult<String> {
        if self.is_empty() {
            return Ok(json.to_string());
        }
        let mut value = serde_json::from_str::<Value>(json).map_err(|error| {
            RuntimeError::new(
                ErrorCode::SecretLeakBlocked,
                format!("cannot guarantee redaction for malformed JSON sink: {error}"),
            )
        })?;
        self.redact_value(&mut value);
        serde_json::to_string(&value).map_err(|error| {
            RuntimeError::new(
                ErrorCode::SecretLeakBlocked,
                format!("failed to serialize redacted JSON sink: {error}"),
            )
        })
    }

    pub fn redact_value(&self, value: &mut Value) {
        if self.is_empty() {
            return;
        }
        match value {
            Value::String(text) => {
                *text = self.redact_text(text);
            }
            Value::Array(items) => {
                for item in items {
                    self.redact_value(item);
                }
            }
            Value::Object(map) => {
                let old = std::mem::take(map);
                let mut redacted = Map::with_capacity(old.len());
                for (key, mut value) in old {
                    self.redact_value(&mut value);
                    redacted.insert(self.redact_text(&key), value);
                }
                *map = redacted;
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    pub fn redact_error(&self, mut error: RuntimeError) -> RuntimeError {
        if self.is_empty() {
            return error;
        }
        error.message = self.redact_text(&error.message);
        self.redact_value(&mut error.details);
        for cause in error.causes.iter_mut() {
            cause.message = self.redact_text(&cause.message);
            self.redact_value(&mut cause.details);
        }
        if let Some(hash) = &mut error.computed_manifest_hash {
            *hash = self.redact_text(hash);
        }
        error
    }

    fn redact_bytes(&self, input: &[u8]) -> Vec<u8> {
        self.scan(input, input.len()).0
    }

    /// Emit every byte whose interpretation can no longer change. Only the
    /// suffix that is still a proper prefix of some secret stays pending, so
    /// whole-buffer longest-first replacement holds across chunk boundaries.
    fn redact_available(&self, pending: &mut Vec<u8>) -> Vec<u8> {
        let (out, consumed) = self.scan(pending, self.undecided_start(pending));
        pending.drain(..consumed);
        out
    }

    fn undecided_start(&self, input: &[u8]) -> usize {
        (0..input.len())
            .find(|&start| {
                let rest = &input[start..];
                self.patterns
                    .iter()
                    .any(|pattern| pattern.len() > rest.len() && pattern.starts_with(rest))
            })
            .unwrap_or(input.len())
    }

    // The limit bounds match starts, not match ends. A longest-first match may
    // consume the undecided tail when it starts in the safe input prefix.
    fn scan(&self, input: &[u8], start_limit: usize) -> (Vec<u8>, usize) {
        if self.is_empty() {
            return (input[..start_limit].to_vec(), start_limit);
        }
        let mut out = Vec::with_capacity(start_limit);
        let mut consumed = 0;
        while consumed < start_limit {
            if let Some(pattern) = self
                .patterns
                .iter()
                .find(|pattern| input[consumed..].starts_with(pattern.as_slice()))
            {
                out.extend_from_slice(REDACTION_TOKEN.as_bytes());
                consumed += pattern.len();
            } else {
                out.push(input[consumed]);
                consumed += 1;
            }
        }
        (out, consumed)
    }
}

pub(crate) const CAPTURE_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(1000);

pub struct RedactedLogRelays {
    stdout: CaptureWorker,
    stderr: CaptureWorker,
}

impl RedactedLogRelays {
    /// Join only workers already known to have finished. Retain results for
    /// checked shutdown; polling never grants completed capture evidence.
    pub(crate) fn check(&self) -> RuntimeResult<()> {
        let stdout = self.stdout.check();
        let stderr = self.stderr.check();
        match (stdout, stderr) {
            (Ok(()), result) | (result, Ok(())) => result,
            (Err(stdout), Err(stderr)) => Err(stdout.with_cause(stderr)),
        }
    }

    /// Settle both writers under one deadline and report the checked outcome:
    /// complete only when both streams reached EOF and every safe byte was
    /// written; incomplete when writers closed with a safe prefix; otherwise
    /// unknown.
    pub fn shutdown(self, deadline: Instant) -> (CaptureOutcome, RuntimeResult<()>) {
        // Publish to both before joining either: progress never extends the budget.
        self.stdout.shutdown_at(deadline);
        self.stderr.shutdown_at(deadline);
        let (stdout_outcome, stdout) = self.stdout.join_outcome();
        let (stderr_outcome, stderr) = self.stderr.join_outcome();
        let result = match (stdout, stderr) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(stdout), Err(stderr)) => Err(stdout.with_cause(stderr)),
        };
        (stdout_outcome.max(stderr_outcome), result)
    }
}

/// The checked capture outcome of one process's retained stdout/stderr files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CaptureOutcome {
    Complete,
    Incomplete,
    Unknown,
}

impl CaptureOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Incomplete => "incomplete",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy)]
enum CapturedStream {
    Stdout,
    Stderr,
}
impl CapturedStream {
    fn incomplete(self) -> RuntimeError {
        leak_blocked(match self {
            Self::Stdout => "captured stdout did not reach EOF before shutdown deadline",
            Self::Stderr => "captured stderr did not reach EOF before shutdown deadline",
        })
    }
}

enum CaptureMode {
    Owned,
    Shutdown(Instant),
}
impl CaptureMode {
    fn aborted(&mut self, control: &Receiver<Instant>) -> bool {
        loop {
            match control.try_recv() {
                Ok(deadline) => {
                    *self = Self::Shutdown(match self {
                        Self::Shutdown(existing) => deadline.min(*existing),
                        Self::Owned => deadline,
                    });
                }
                Err(TryRecvError::Disconnected) => return true,
                Err(TryRecvError::Empty) => break,
            }
        }
        matches!(self, Self::Shutdown(deadline) if Instant::now() >= *deadline)
    }
}

enum CaptureCompletion {
    Eof,
    Incomplete,
}

enum WorkerState {
    Running(JoinHandle<RuntimeResult<CaptureCompletion>>),
    Finished(CaptureOutcome, RuntimeResult<()>),
}

struct CaptureWorker {
    stream: CapturedStream,
    control: Sender<Instant>,
    // None only during consuming join/drop or an internal state transition.
    state: RefCell<Option<WorkerState>>,
}
impl CaptureWorker {
    fn shutdown_at(&self, deadline: Instant) {
        let _ = self.control.send(deadline);
    }
    fn checked_join(
        &self,
        handle: JoinHandle<RuntimeResult<CaptureCompletion>>,
    ) -> (CaptureOutcome, RuntimeResult<()>) {
        match handle.join() {
            Ok(Ok(CaptureCompletion::Eof)) => (CaptureOutcome::Complete, Ok(())),
            Ok(Ok(CaptureCompletion::Incomplete)) => {
                (CaptureOutcome::Incomplete, Err(self.stream.incomplete()))
            }
            Ok(Err(error)) => (CaptureOutcome::Unknown, Err(error)),
            Err(_) => (
                CaptureOutcome::Unknown,
                Err(leak_blocked(
                    "redaction relay panicked before proving captured output was scrubbed",
                )),
            ),
        }
    }
    fn check(&self) -> RuntimeResult<()> {
        let mut state = self.state.borrow_mut();
        if matches!(state.as_ref(), Some(WorkerState::Running(handle)) if handle.is_finished()) {
            let Some(WorkerState::Running(handle)) = state.take() else {
                unreachable!()
            };
            let (outcome, result) = self.checked_join(handle);
            *state = Some(WorkerState::Finished(outcome, result));
        }
        match state.as_ref().expect("capture state is owned") {
            WorkerState::Running(_) => Ok(()),
            WorkerState::Finished(_, result) => result.clone(),
        }
    }
    fn join(self) -> RuntimeResult<()> {
        self.join_outcome().1
    }
    fn join_outcome(self) -> (CaptureOutcome, RuntimeResult<()>) {
        let state = self
            .state
            .borrow_mut()
            .take()
            .expect("capture state is consumed once");
        match state {
            WorkerState::Running(handle) => self.checked_join(handle),
            WorkerState::Finished(outcome, result) => (outcome, result),
        }
    }
}
impl Drop for CaptureWorker {
    fn drop(&mut self) {
        if let Some(WorkerState::Running(handle)) = self.state.get_mut().take() {
            self.shutdown_at(Instant::now());
            let _ = handle.join();
        }
    }
}

pub struct RedactedChildOutput {
    pub stdout: Stdio,
    pub stderr: Stdio,
    pub relays: RedactedLogRelays,
}

#[derive(Clone, Copy)]
pub(crate) enum LogFileMode {
    New,
    Replace,
}

pub(crate) fn child_output(
    stdout_path: &Path,
    stderr_path: &Path,
    redactor: &Redactor,
    mode: LogFileMode,
) -> RuntimeResult<RedactedChildOutput> {
    let (stdout, stdout_relay) =
        redacted_stdio(stdout_path, redactor, CapturedStream::Stdout, mode)?;
    let (stderr, stderr_relay) =
        match redacted_stdio(stderr_path, redactor, CapturedStream::Stderr, mode) {
            Ok(output) => output,
            Err(error) => {
                drop(stdout);
                stdout_relay.shutdown_at(Instant::now() + CAPTURE_SHUTDOWN_TIMEOUT);
                return Err(match stdout_relay.join() {
                    Ok(()) => error,
                    Err(capture) => capture.with_cause(error),
                });
            }
        };
    Ok(RedactedChildOutput {
        stdout,
        stderr,
        relays: RedactedLogRelays {
            stdout: stdout_relay,
            stderr: stderr_relay,
        },
    })
}

fn redacted_stdio(
    path: &Path,
    redactor: &Redactor,
    stream: CapturedStream,
    mode: LogFileMode,
) -> RuntimeResult<(Stdio, CaptureWorker)> {
    let mut fds = [0; 2];
    #[cfg(target_os = "linux")]
    let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
    let result = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if result != 0 {
        return Err(leak_blocked(format!(
            "failed to create redaction pipe for {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    let read_end = unsafe { File::from_raw_fd(fds[0]) };
    let write_end = unsafe { File::from_raw_fd(fds[1]) };
    for fd in [read_end.as_raw_fd(), write_end.as_raw_fd()] {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(leak_blocked("failed to set capture pipe close-on-exec"));
        }
    }
    let flags = unsafe { libc::fcntl(read_end.as_raw_fd(), libc::F_GETFL) };
    if flags == -1
        || unsafe {
            libc::fcntl(
                read_end.as_raw_fd(),
                libc::F_SETFL,
                flags | libc::O_NONBLOCK,
            )
        } == -1
    {
        return Err(leak_blocked("failed to set capture pipe nonblocking"));
    }
    let writer = create_log_file(path, !redactor.is_empty(), mode)?;
    let redactor = redactor.clone();
    let (control, receiver) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("nixfied-capture".into())
        .spawn(move || redact_stream(read_end, writer, redactor, receiver))
        .map_err(|error| leak_blocked(format!("failed to start redaction relay: {error}")))?;
    Ok((
        Stdio::from(write_end),
        CaptureWorker {
            stream,
            control,
            state: RefCell::new(Some(WorkerState::Running(handle))),
        },
    ))
}

fn redact_stream(
    reader: File,
    writer: File,
    redactor: Redactor,
    control: Receiver<Instant>,
) -> RuntimeResult<CaptureCompletion> {
    redact_stream_with_poll(reader, writer, redactor, control, |fd, timeout| unsafe {
        libc::poll(fd, 1, timeout)
    })
}

fn redact_stream_with_poll(
    mut reader: File,
    mut writer: File,
    redactor: Redactor,
    control: Receiver<Instant>,
    mut poll: impl FnMut(&mut libc::pollfd, i32) -> i32,
) -> RuntimeResult<CaptureCompletion> {
    let mut pending = Vec::new();
    let mut buf = [0; 8192];
    let mut mode = CaptureMode::Owned;
    let completion = loop {
        if mode.aborted(&control) {
            break CaptureCompletion::Incomplete;
        }
        let wait = match mode {
            CaptureMode::Shutdown(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    break CaptureCompletion::Incomplete;
                }
                (deadline - now).min(Duration::from_millis(10))
            }
            CaptureMode::Owned => Duration::from_millis(10),
        };
        let mut pollfd = libc::pollfd {
            fd: reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let polled = poll(&mut pollfd, wait.as_millis() as i32);
        if polled == -1 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(leak_blocked(format!(
                "failed to read child output for redaction: {error}"
            )));
        }
        if polled == 0 {
            continue;
        }
        // A shutdown message can arrive during poll. Observe it before any
        // subsequent read, including EOF, so expired shutdown wins over readiness.
        if mode.aborted(&control) {
            break CaptureCompletion::Incomplete;
        }
        let read = match reader.read(&mut buf) {
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) => {
                return Err(leak_blocked(format!(
                    "failed to read child output for redaction: {error}"
                )));
            }
        };
        if read == 0 {
            break CaptureCompletion::Eof;
        }
        pending.extend_from_slice(&buf[..read]);
        let redacted = redactor.redact_available(&mut pending);
        writer.write_all(&redacted).map_err(|error| {
            leak_blocked(format!("failed to write redacted child output: {error}"))
        })?;
    };
    drop(reader);
    if matches!(completion, CaptureCompletion::Eof) && !pending.is_empty() {
        let redacted = redactor.redact_bytes(&pending);
        writer.write_all(&redacted).map_err(|error| {
            leak_blocked(format!(
                "failed to write final redacted child output: {error}"
            ))
        })?;
    }
    // Incomplete capture discards the undecided tail; only safe bytes are flushed.
    writer
        .flush()
        .map_err(|error| leak_blocked(format!("failed to flush redacted child output: {error}")))?;
    Ok(completion)
}

fn create_log_file(path: &Path, redacted: bool, mode: LogFileMode) -> RuntimeResult<File> {
    match mode {
        LogFileMode::New => File::create_new(path),
        LogFileMode::Replace => File::create(path),
    }
    .map_err(|error| {
        let message = format!("failed to create log file {}: {error}", path.display());
        if redacted {
            leak_blocked(message)
        } else {
            RuntimeError::new(ErrorCode::StateUnwritable, message)
        }
    })
}

fn leak_blocked(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(ErrorCode::SecretLeakBlocked, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CaptureFixture(std::path::PathBuf);
    impl CaptureFixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "nixfied-capture-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn worker(
            &self,
            stream: CapturedStream,
            redactor: Redactor,
            writable: bool,
        ) -> (
            CaptureWorker,
            std::os::unix::net::UnixStream,
            std::path::PathBuf,
        ) {
            let name = match stream {
                CapturedStream::Stdout => "stdout",
                CapturedStream::Stderr => "stderr",
            };
            let path = self.0.join(name);
            let writer = File::create(&path).unwrap();
            let writer = if writable {
                writer
            } else {
                drop(writer);
                File::open(&path).unwrap()
            };
            let (reader, sender) = std::os::unix::net::UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap();
            let reader = File::from(std::os::fd::OwnedFd::from(reader));
            let (control, receiver) = mpsc::channel();
            let handle = thread::spawn(move || redact_stream(reader, writer, redactor, receiver));
            (
                CaptureWorker {
                    stream,
                    control,
                    state: RefCell::new(Some(WorkerState::Running(handle))),
                },
                sender,
                path,
            )
        }
    }
    impl Drop for CaptureFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn eof_finalizes_both_streams_and_preserves_binary_empty_redactor_output() {
        let fixture = CaptureFixture::new();
        let (stdout, mut out, out_path) = fixture.worker(
            CapturedStream::Stdout,
            Redactor {
                patterns: vec![b"abcdef".to_vec()],
            },
            true,
        );
        let (stderr, mut err, err_path) =
            fixture.worker(CapturedStream::Stderr, Redactor::empty(), true);
        out.write_all(b"abcdef-tail").unwrap();
        err.write_all(b"\x00\xffbinary").unwrap();
        drop(out);
        drop(err);
        RedactedLogRelays { stdout, stderr }
            .shutdown(Instant::now() + CAPTURE_SHUTDOWN_TIMEOUT)
            .1
            .unwrap();
        assert_eq!(std::fs::read(out_path).unwrap(), b"[REDACTED]-tail");
        assert_eq!(std::fs::read(err_path).unwrap(), b"\x00\xffbinary");
    }

    #[test]
    fn shutdown_at_poll_boundary_rejects_readable_eof() {
        let fixture = CaptureFixture::new();
        let path = fixture.0.join("stdout");
        let (reader, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let reader = File::from(std::os::fd::OwnedFd::from(reader));
        let (sender, receiver) = mpsc::channel();
        let mut peer = Some(peer);
        let completion = redact_stream_with_poll(
            reader,
            File::create(&path).unwrap(),
            Redactor::empty(),
            receiver,
            |fd, timeout| {
                // Queue expiry after the loop's initial control check and make
                // real EOF readable. The production post-poll check must win.
                sender.send(Instant::now()).unwrap();
                drop(peer.take().expect("capture must stop after the first poll"));
                let ready = unsafe { libc::poll(fd, 1, timeout) };
                assert_eq!(ready, 1);
                ready
            },
        )
        .unwrap();
        assert!(matches!(completion, CaptureCompletion::Incomplete));
        assert!(std::fs::read(path).unwrap().is_empty());
    }

    #[test]
    fn partial_capture_construction_closes_stdout_and_preserves_file_error_classes() {
        for secret in [false, true] {
            let fixture = CaptureFixture::new();
            let stdout = fixture.0.join("stdout");
            let redactor = if secret {
                Redactor {
                    patterns: vec![b"secret".to_vec()],
                }
            } else {
                Redactor::empty()
            };
            let error = child_output(&stdout, &fixture.0, &redactor, LogFileMode::Replace)
                .err()
                .unwrap();
            assert_eq!(
                error.code,
                if secret {
                    ErrorCode::SecretLeakBlocked
                } else {
                    ErrorCode::StateUnwritable
                }
            );
            assert!(std::fs::read(&stdout).unwrap().is_empty());
            // Construction returned only after the already-created stdout worker joined.
            std::fs::remove_file(stdout).unwrap();
        }
    }

    #[test]
    fn owned_capture_joins_and_preserves_redacted_eof_tail() {
        let fixture = CaptureFixture::new();
        let (worker, mut writer, path) = fixture.worker(
            CapturedStream::Stdout,
            Redactor {
                patterns: vec![b"secret".to_vec()],
            },
            true,
        );
        writer.write_all(b"secretsecret-tail").unwrap();
        drop(writer);
        worker.join().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"[REDACTED][REDACTED]-tail");
    }

    #[test]
    fn failed_worker_is_polled_without_waiting_for_the_other_stream() {
        let fixture = CaptureFixture::new();
        let (stdout, mut out, _) = fixture.worker(CapturedStream::Stdout, Redactor::empty(), false);
        let (stderr, err, _) = fixture.worker(CapturedStream::Stderr, Redactor::empty(), true);
        let relays = RedactedLogRelays { stdout, stderr };
        assert!(relays.check().is_ok());
        out.write_all(b"failure").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let error = loop {
            match relays.check() {
                Err(error) => break error,
                Ok(()) => {
                    assert!(
                        Instant::now() < deadline,
                        "capture failure was not observed"
                    );
                    thread::sleep(Duration::from_millis(1));
                }
            }
        };
        assert_eq!(error.code, ErrorCode::SecretLeakBlocked);
        assert!(error.message.contains("failed to write"));
        assert_eq!(relays.check().unwrap_err().message, error.message);
        drop(err);
        let settled = relays
            .shutdown(Instant::now() + CAPTURE_SHUTDOWN_TIMEOUT)
            .1
            .unwrap_err();
        assert_eq!(settled.message, error.message);
    }

    #[test]
    fn incremental_redaction_holds_back_only_a_possible_secret_prefix() {
        let redactor = Redactor {
            patterns: vec![b"secret-longer".to_vec(), b"secret".to_vec()],
        };
        for (input, emitted, pending) in [
            (&b"plain text"[..], &b"plain text"[..], &b""[..]),
            (b"value: sec", b"value: ", b"sec"),
            // "secret" matches now but "secret-longer" may still arrive.
            (b"x secret-lo", b"x ", b"secret-lo"),
            (b"x secret!", b"x [REDACTED]!", b""),
            (b"x secret-longer tail", b"x [REDACTED] tail", b""),
        ] {
            let mut buffer = input.to_vec();
            assert_eq!(redactor.redact_available(&mut buffer), emitted, "{input:?}");
            assert_eq!(buffer, pending, "{input:?}");
        }
        // Chunk-split input redacts exactly like the whole buffer.
        let whole = b"a secret-longer b secret c secre";
        for split in 0..whole.len() {
            let mut pending = whole[..split].to_vec();
            let mut output = redactor.redact_available(&mut pending);
            pending.extend_from_slice(&whole[split..]);
            output.extend(redactor.redact_available(&mut pending));
            output.extend(redactor.redact_bytes(&pending));
            assert_eq!(output, redactor.redact_bytes(whole), "split {split}");
        }
    }

    #[test]
    fn retained_writers_share_one_deadline_and_discard_partial_secret_tails() {
        let fixture = CaptureFixture::new();
        let redactor = Redactor {
            patterns: vec![b"abcdef".to_vec()],
        };
        let (stdout, mut out, out_path) =
            fixture.worker(CapturedStream::Stdout, redactor.clone(), true);
        let (stderr, mut err, err_path) = fixture.worker(CapturedStream::Stderr, redactor, true);
        out.write_all(b"safe-data-abc").unwrap();
        err.write_all(b"safe-data-abc").unwrap();
        let (done, result) = mpsc::channel();
        let started = Instant::now();
        thread::spawn(move || {
            done.send(
                RedactedLogRelays { stdout, stderr }
                    .shutdown(Instant::now() + Duration::from_millis(100)),
            )
            .unwrap();
        });
        let (outcome, result) = result.recv_timeout(Duration::from_secs(3)).unwrap();
        // Writers closed with a safe prefix: incomplete, never complete.
        assert_eq!(outcome, CaptureOutcome::Incomplete);
        let error = result.unwrap_err();
        assert!(started.elapsed() < Duration::from_millis(1000));
        assert_eq!(
            error.message,
            "captured stdout did not reach EOF before shutdown deadline"
        );
        assert_eq!(error.causes.len(), 1);
        assert_eq!(
            error.causes[0].message,
            "captured stderr did not reach EOF before shutdown deadline"
        );
        assert_eq!(std::fs::read(&out_path).unwrap(), b"safe-data-");
        assert_eq!(std::fs::read(&err_path).unwrap(), b"safe-data-");
        assert!(out.write_all(b"def").is_err());
        assert!(err.write_all(b"def").is_err());
        assert_eq!(std::fs::read(out_path).unwrap(), b"safe-data-");
        assert_eq!(std::fs::read(err_path).unwrap(), b"safe-data-");
    }

    #[test]
    fn first_worker_error_still_joins_the_other_continuously_readable_worker() {
        let fixture = CaptureFixture::new();
        let (stdout, mut out, _) = fixture.worker(CapturedStream::Stdout, Redactor::empty(), false);
        let (stderr, mut err, err_path) =
            fixture.worker(CapturedStream::Stderr, Redactor::empty(), true);
        out.write_all(b"cannot write this").unwrap();
        drop(out);
        let writer = thread::spawn(move || while err.write_all(&[b'x'; 8192]).is_ok() {});
        let (done, result) = mpsc::channel();
        thread::spawn(move || {
            done.send(
                RedactedLogRelays { stdout, stderr }
                    .shutdown(Instant::now() + Duration::from_millis(100))
                    .1,
            )
            .unwrap();
        });
        let error = result
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap_err();
        assert!(
            error
                .message
                .starts_with("failed to write redacted child output:")
        );
        assert_eq!(error.causes.len(), 1);
        assert_eq!(
            error.causes[0].message,
            "captured stderr did not reach EOF before shutdown deadline"
        );
        writer.join().unwrap();
        let captured = std::fs::read(&err_path).unwrap();
        assert!(!captured.is_empty());
        assert!(captured.iter().all(|byte| *byte == b'x'));
        assert_eq!(std::fs::read(err_path).unwrap(), captured);
    }

    #[test]
    fn redacts_text_and_json_values() {
        let redactor = Redactor {
            patterns: vec![b"super-secret".to_vec()],
        };
        assert_eq!(
            redactor.redact_text("token=super-secret"),
            "token=[REDACTED]"
        );
        assert_eq!(
            redactor
                .redact_json_str(r#"{"token":"super-secret","nested":["xsuper-secretx"]}"#)
                .unwrap(),
            r#"{"nested":["x[REDACTED]x"],"token":"[REDACTED]"}"#
        );
    }

    #[test]
    fn longest_matches_preserve_literal_bytes_at_every_chunk_split() {
        let redactor = Redactor {
            patterns: vec![b"abcde".to_vec(), b"abc".to_vec()],
        };
        let input = b"\xffabcdeabc\0abxabcde";
        let expected = b"\xff[REDACTED][REDACTED]\0abx[REDACTED]";
        assert_eq!(redactor.redact_bytes(input), expected);
        for first in 0..=input.len() {
            for second in first..=input.len() {
                let mut pending = Vec::new();
                let mut actual = Vec::new();
                for chunk in [&input[..first], &input[first..second], &input[second..]] {
                    pending.extend_from_slice(chunk);
                    actual.extend(redactor.redact_available(&mut pending));
                }
                actual.extend(redactor.redact_bytes(&pending));
                assert_eq!(actual, expected, "splits {first}, {second}");
            }
        }
        assert!(redactor.redact_bytes(b"").is_empty());
        let empty = Redactor::empty();
        assert_eq!(empty.redact_bytes(input), input);
        let mut pending = input.to_vec();
        assert_eq!(empty.redact_available(&mut pending), input);
        assert!(pending.is_empty());
    }

    #[test]
    fn redacts_streaming_matches_across_chunks() {
        let redactor = Redactor {
            patterns: vec![b"abcdef".to_vec()],
        };
        let mut pending = b"abc".to_vec();
        pending.extend_from_slice(b"def");
        assert_eq!(
            redactor.redact_available(&mut pending),
            b"[REDACTED]".to_vec()
        );
        assert!(pending.is_empty());
    }
}
