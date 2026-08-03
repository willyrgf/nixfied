//! Runtime-owned replay of redacted task evidence.
//!
//! A replay ticket owns the already-open evidence sources.  Opening the sources
//! before terminal registry transitions is what makes replay independent of
//! later cleanup of their directory entries.  The two streams are deliberately
//! independent: a failure in one stream must not prevent the other stream from
//! being drained and reported.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;

use serde::Serialize;

const COPY_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceMode {
    CaptureOnly,
    ReplaySelected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectionOperation {
    Open,
    Read,
    Write,
    Flush,
    Join,
}

/// A redaction-safe description of one replay failure.
///
/// It intentionally stores an error kind rather than an operating-system error
/// string.  Paths are runtime-owned evidence paths and the byte count is the
/// only progress detail needed by the public projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionIssue {
    stream: OutputStream,
    operation: ProjectionOperation,
    kind: String,
    path: PathBuf,
    bytes_written: u64,
}

impl ProjectionIssue {
    pub fn stream(&self) -> OutputStream {
        self.stream
    }

    pub fn operation(&self) -> ProjectionOperation {
        self.operation
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    fn io(
        stream: OutputStream,
        operation: ProjectionOperation,
        path: &Path,
        error: &io::Error,
        bytes_written: u64,
    ) -> Self {
        Self {
            stream,
            operation,
            kind: io_kind(error).to_string(),
            path: path.to_path_buf(),
            bytes_written,
        }
    }

    fn worker_panic(stream: OutputStream, path: PathBuf) -> Self {
        Self {
            stream,
            operation: ProjectionOperation::Join,
            kind: "worker-panic".to_string(),
            path,
            bytes_written: 0,
        }
    }

    fn worker_spawn(stream: OutputStream, path: PathBuf) -> Self {
        Self {
            stream,
            operation: ProjectionOperation::Join,
            kind: "worker-spawn".to_string(),
            path,
            bytes_written: 0,
        }
    }
}

#[derive(Debug)]
enum ReplaySource {
    Ready { file: File, path: PathBuf },
    OpenFailed(ProjectionIssue),
}

impl ReplaySource {
    fn open(stream: OutputStream, path: &Path) -> Self {
        match File::open(path) {
            Ok(file) => Self::Ready {
                file,
                path: path.to_path_buf(),
            },
            Err(error) => Self::OpenFailed(ProjectionIssue::io(
                stream,
                ProjectionOperation::Open,
                path,
                &error,
                0,
            )),
        }
    }

    fn path(&self) -> PathBuf {
        match self {
            Self::Ready { path, .. } => path.clone(),
            Self::OpenFailed(issue) => issue.path.clone(),
        }
    }
}

/// Move-only evidence sources for one directly selected task.
#[derive(Debug)]
pub struct ReplayTicket {
    stdout: ReplaySource,
    stderr: ReplaySource,
}

impl ReplayTicket {
    /// Open both sources independently.  One failed open is retained as a
    /// source-local issue so the other stream remains replayable.
    pub fn open(stdout: &Path, stderr: &Path) -> Self {
        Self {
            stdout: ReplaySource::open(OutputStream::Stdout, stdout),
            stderr: ReplaySource::open(OutputStream::Stderr, stderr),
        }
    }

    /// Replay both streams concurrently and consume the ticket exactly once.
    pub fn replay(self, sinks: ReplaySinks) -> ReplayReport {
        let stdout_path = self.stdout.path();
        let stderr_path = self.stderr.path();
        let mut issues = Vec::new();
        let stdout = thread::Builder::new()
            .name("nixfied-replay-stdout".to_string())
            .spawn(move || replay_source(OutputStream::Stdout, self.stdout, sinks.stdout));
        let stderr = thread::Builder::new()
            .name("nixfied-replay-stderr".to_string())
            .spawn(move || replay_source(OutputStream::Stderr, self.stderr, sinks.stderr));
        collect_worker(&mut issues, OutputStream::Stdout, stdout_path, stdout);
        collect_worker(&mut issues, OutputStream::Stderr, stderr_path, stderr);
        ReplayReport { issues }
    }
}

fn collect_worker(
    issues: &mut Vec<ProjectionIssue>,
    stream: OutputStream,
    path: PathBuf,
    worker: io::Result<thread::JoinHandle<Vec<ProjectionIssue>>>,
) {
    match worker {
        Ok(worker) => match worker.join() {
            Ok(worker_issues) => issues.extend(worker_issues),
            Err(_) => issues.push(ProjectionIssue::worker_panic(stream, path)),
        },
        Err(_) => issues.push(ProjectionIssue::worker_spawn(stream, path)),
    }
}

fn replay_source(
    stream: OutputStream,
    source: ReplaySource,
    mut sink: Box<dyn Write + Send>,
) -> Vec<ProjectionIssue> {
    match source {
        ReplaySource::Ready { file, path } => copy_reader(stream, &path, file, &mut sink),
        ReplaySource::OpenFailed(issue) => vec![issue],
    }
}

/// The two output sinks used by a replay.  Each sink is moved to its own worker
/// so a slow or failed stderr destination cannot block stdout progress.
pub struct ReplaySinks {
    stdout: Box<dyn Write + Send>,
    stderr: Box<dyn Write + Send>,
}

impl ReplaySinks {
    pub fn new(stdout: impl Write + Send + 'static, stderr: impl Write + Send + 'static) -> Self {
        Self {
            stdout: Box::new(stdout),
            stderr: Box::new(stderr),
        }
    }

    pub fn stdio() -> Self {
        Self::new(io::stdout(), io::stderr())
    }
}

#[derive(Debug, Default)]
pub struct ReplayReport {
    issues: Vec<ProjectionIssue>,
}

impl ReplayReport {
    pub fn is_success(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn issues(&self) -> &[ProjectionIssue] {
        &self.issues
    }

    pub fn into_issues(self) -> Vec<ProjectionIssue> {
        self.issues
    }
}

fn copy_reader<R: Read, W: Write>(
    stream: OutputStream,
    path: &Path,
    mut reader: R,
    writer: &mut W,
) -> Vec<ProjectionIssue> {
    let mut buffer = [0_u8; COPY_BUFFER_SIZE];
    let mut bytes_written = 0_u64;
    let mut issue = None;
    'read: loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                issue = Some(ProjectionIssue::io(
                    stream,
                    ProjectionOperation::Read,
                    path,
                    &error,
                    bytes_written,
                ));
                break;
            }
        };
        if read == 0 {
            break;
        }

        let mut offset = 0;
        while offset < read {
            match writer.write(&buffer[offset..read]) {
                Ok(written) if written > 0 => {
                    offset += written;
                    bytes_written = bytes_written.saturating_add(written as u64);
                }
                Ok(_) => {
                    issue = Some(ProjectionIssue {
                        stream,
                        operation: ProjectionOperation::Write,
                        kind: "write-zero".to_string(),
                        path: path.to_path_buf(),
                        bytes_written,
                    });
                    break 'read;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    issue = Some(ProjectionIssue::io(
                        stream,
                        ProjectionOperation::Write,
                        path,
                        &error,
                        bytes_written,
                    ));
                    break 'read;
                }
            }
        }
    }
    let mut issues = issue.into_iter().collect::<Vec<_>>();
    issues.extend(flush_writer(stream, path, writer, bytes_written));
    issues
}

fn flush_writer<W: Write>(
    stream: OutputStream,
    path: &Path,
    writer: &mut W,
    bytes_written: u64,
) -> Vec<ProjectionIssue> {
    loop {
        match writer.flush() {
            Ok(()) => return Vec::new(),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return vec![ProjectionIssue::io(
                    stream,
                    ProjectionOperation::Flush,
                    path,
                    &error,
                    bytes_written,
                )];
            }
        }
    }
}

fn io_kind(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::BrokenPipe => "broken-pipe",
        io::ErrorKind::NotFound => "not-found",
        io::ErrorKind::PermissionDenied => "permission-denied",
        io::ErrorKind::ConnectionAborted => "connection-aborted",
        io::ErrorKind::ConnectionReset => "connection-reset",
        io::ErrorKind::ConnectionRefused => "connection-refused",
        io::ErrorKind::InvalidData => "invalid-data",
        io::ErrorKind::UnexpectedEof => "unexpected-eof",
        io::ErrorKind::WouldBlock => "would-block",
        io::ErrorKind::TimedOut => "timed-out",
        _ => "io",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Clone)]
    struct SharedWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
        max_write: usize,
    }

    impl Write for SharedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let amount = bytes.len().min(self.max_write);
            self.bytes
                .lock()
                .expect("writer lock")
                .extend_from_slice(&bytes[..amount]);
            Ok(amount)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FailingReader {
        bytes: Vec<u8>,
        failed: bool,
    }

    impl Read for FailingReader {
        fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
            if !self.bytes.is_empty() {
                let amount = self.bytes.len().min(target.len());
                target[..amount].copy_from_slice(&self.bytes[..amount]);
                self.bytes.drain(..amount);
                return Ok(amount);
            }
            if !self.failed {
                self.failed = true;
                return Err(io::Error::new(
                    ErrorKind::PermissionDenied,
                    "test read fault",
                ));
            }
            Ok(0)
        }
    }

    struct FailingWriter {
        bytes: Vec<u8>,
        fail_after: usize,
    }

    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.bytes.len() >= self.fail_after {
                return Err(io::Error::new(ErrorKind::BrokenPipe, "test write fault"));
            }
            let amount = (self.fail_after - self.bytes.len()).min(bytes.len());
            self.bytes.extend_from_slice(&bytes[..amount]);
            Ok(amount)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn temp_path(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "nixfied-output-{label}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[test]
    fn replays_both_streams_exactly_with_partial_writes() {
        let stdout_path = temp_path("stdout");
        let stderr_path = temp_path("stderr");
        std::fs::write(&stdout_path, [0, 1, 2, 0, 255, 10]).expect("stdout evidence");
        std::fs::write(&stderr_path, b"stderr without a newline").expect("stderr evidence");

        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let report = ReplayTicket::open(&stdout_path, &stderr_path).replay(ReplaySinks::new(
            SharedWriter {
                bytes: Arc::clone(&stdout),
                max_write: 2,
            },
            SharedWriter {
                bytes: Arc::clone(&stderr),
                max_write: 3,
            },
        ));

        assert!(
            report.is_success(),
            "unexpected issues: {:?}",
            report.issues()
        );
        assert_eq!(
            &*stdout.lock().expect("stdout lock"),
            &[0, 1, 2, 0, 255, 10]
        );
        assert_eq!(
            &*stderr.lock().expect("stderr lock"),
            b"stderr without a newline"
        );
        std::fs::remove_file(stdout_path).expect("remove stdout evidence");
        std::fs::remove_file(stderr_path).expect("remove stderr evidence");
    }

    #[test]
    fn an_open_failure_does_not_block_the_other_stream() {
        let stdout_path = temp_path("missing-stdout");
        let stderr_path = temp_path("present-stderr");
        std::fs::write(&stderr_path, b"still available").expect("stderr evidence");
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let report = ReplayTicket::open(&stdout_path, &stderr_path).replay(ReplaySinks::new(
            Vec::<u8>::new(),
            SharedWriter {
                bytes: Arc::clone(&stderr),
                max_write: 64,
            },
        ));

        assert_eq!(report.issues().len(), 1);
        assert_eq!(report.issues()[0].stream, OutputStream::Stdout);
        assert_eq!(report.issues()[0].operation, ProjectionOperation::Open);
        assert_eq!(report.issues()[0].kind, "not-found");
        assert_eq!(&*stderr.lock().expect("stderr lock"), b"still available");
        std::fs::remove_file(stderr_path).expect("remove stderr evidence");
    }

    #[test]
    fn read_and_write_faults_are_typed_and_count_progress() {
        let read_issues = copy_reader(
            OutputStream::Stdout,
            Path::new("/evidence/stdout.log"),
            FailingReader {
                bytes: b"partial".to_vec(),
                failed: false,
            },
            &mut Vec::new(),
        );
        assert_eq!(read_issues[0].operation, ProjectionOperation::Read);
        assert_eq!(read_issues[0].kind, "permission-denied");
        assert_eq!(read_issues[0].bytes_written, 7);

        let mut writer = FailingWriter {
            bytes: Vec::new(),
            fail_after: 3,
        };
        let write_issues = copy_reader(
            OutputStream::Stderr,
            Path::new("/evidence/stderr.log"),
            &b"abcdef"[..],
            &mut writer,
        );
        assert_eq!(write_issues[0].operation, ProjectionOperation::Write);
        assert_eq!(write_issues[0].kind, "broken-pipe");
        assert_eq!(write_issues[0].bytes_written, 3);
    }

    #[test]
    fn interrupted_read_and_write_are_retried() {
        struct InterruptedOnce<R> {
            inner: R,
            interrupted: bool,
        }

        impl<R: Read> Read for InterruptedOnce<R> {
            fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::Error::from(ErrorKind::Interrupted));
                }
                self.inner.read(target)
            }
        }

        struct InterruptedWriter {
            bytes: Vec<u8>,
            interrupted: bool,
        }

        impl Write for InterruptedWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::Error::from(ErrorKind::Interrupted));
                }
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = InterruptedWriter {
            bytes: Vec::new(),
            interrupted: false,
        };
        let issues = copy_reader(
            OutputStream::Stdout,
            Path::new("/evidence/stdout.log"),
            InterruptedOnce {
                inner: &b"retry me"[..],
                interrupted: false,
            },
            &mut writer,
        );
        assert!(issues.is_empty());
        assert_eq!(writer.bytes, b"retry me");
    }
}
