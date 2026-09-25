//! Output vocabulary shared by the session owner and its command-owned
//! presenter. The session writes retained redacted evidence; the presenter,
//! never the session, writes caller streams while session duties remain.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectionOperation {
    Open,
    Read,
    Write,
    Flush,
    Join,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionDiagnostic<'a> {
    pub stream: &'a OutputStream,
    pub operation: &'a ProjectionOperation,
    pub kind: &'a str,
    pub path: &'a str,
    pub bytes_written: u64,
}

/// A redaction-safe description of one delivery failure. It stores an error
/// kind rather than an operating-system error string; paths are runtime-owned
/// evidence paths or the caller stream name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectionIssue {
    pub stream: OutputStream,
    pub operation: ProjectionOperation,
    pub kind: String,
    pub path: PathBuf,
    pub bytes_written: u64,
}

impl ProjectionIssue {
    pub fn io(
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

    pub fn named(stream: OutputStream, kind: &str, path: &Path) -> Self {
        Self {
            stream,
            operation: ProjectionOperation::Join,
            kind: kind.to_string(),
            path: path.to_path_buf(),
            bytes_written: 0,
        }
    }
}

/// How one retained source is presented while its session runs. The owner
/// decides this when registering the source; presentation never parses labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourcePresentation {
    /// The directly selected leaf task: exact bytes in `task-output` mode.
    Selected,
    /// Other executed tasks, preparation, and services: human live output.
    Shown,
    /// Replace-on-retry probe logs stay outside the live display.
    Hidden,
}

impl SourcePresentation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Shown => "shown",
            Self::Hidden => "hidden",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "selected" => Some(Self::Selected),
            "shown" => Some(Self::Shown),
            "hidden" => Some(Self::Hidden),
            _ => None,
        }
    }
}

/// One process's retained stdout/stderr evidence: absolute paths for the
/// writer and the relative placement recorded as the machine locator.
#[derive(Debug, Clone)]
pub struct EvidenceSource {
    pub label: String,
    pub presentation: SourcePresentation,
    pub stdout: PathBuf,
    pub stderr: PathBuf,
    pub stdout_relative: String,
    pub stderr_relative: String,
}

impl EvidenceSource {
    /// `logs_dir` is always `<run>/logs`; recorded paths are relative to `<run>`.
    pub fn in_logs(
        logs_dir: &Path,
        label: impl Into<String>,
        presentation: SourcePresentation,
        stem: &str,
    ) -> Self {
        let stdout_name = format!("{stem}.stdout.log");
        let stderr_name = format!("{stem}.stderr.log");
        Self {
            label: label.into(),
            presentation,
            stdout: logs_dir.join(&stdout_name),
            stderr: logs_dir.join(&stderr_name),
            stdout_relative: format!("logs/{stdout_name}"),
            stderr_relative: format!("logs/{stderr_name}"),
        }
    }
}

/// The run-owned diagnostic source: runtime progress lines presented on stderr.
pub const DIAGNOSTIC_SOURCE: &str = "diagnostics.log";

/// Collect delivery failures into the public projection error.
pub fn projection_error(message: &str, issues: Vec<ProjectionIssue>) -> crate::error::RuntimeError {
    let projections = issues
        .into_iter()
        .map(|issue| {
            let path = issue.path.to_string_lossy();
            json!(ProjectionDiagnostic {
                stream: &issue.stream,
                operation: &issue.operation,
                kind: &issue.kind,
                path: path.as_ref(),
                bytes_written: issue.bytes_written,
            })
        })
        .collect::<Vec<_>>();
    crate::error::RuntimeError::new(crate::error::ErrorCode::OutputProjectionFailed, message)
        .with_detail("projections", projections)
}

/// Command-scoped writes after slot release keep their narrower kind mapping.
pub fn output_projection_io_error(
    stream: OutputStream,
    operation: ProjectionOperation,
    path: &str,
    error: io::Error,
) -> crate::error::RuntimeError {
    let kind = match error.kind() {
        io::ErrorKind::BrokenPipe => "broken-pipe",
        io::ErrorKind::PermissionDenied => "permission-denied",
        io::ErrorKind::Interrupted => "interrupted",
        _ => "io",
    };
    crate::error::RuntimeError::new(
        crate::error::ErrorCode::OutputProjectionFailed,
        "runtime output projection failed",
    )
    .with_detail(
        "projections",
        vec![ProjectionDiagnostic {
            stream: &stream,
            operation: &operation,
            kind,
            path,
            bytes_written: 0,
        }],
    )
}

pub fn io_kind(error: &io::Error) -> &'static str {
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

    #[test]
    fn summary_projection_preserves_safe_kind_policy() {
        for (kind, summary_kind, delivery_kind) in [
            (io::ErrorKind::BrokenPipe, "broken-pipe", "broken-pipe"),
            (
                io::ErrorKind::PermissionDenied,
                "permission-denied",
                "permission-denied",
            ),
            (io::ErrorKind::Interrupted, "interrupted", "io"),
            (io::ErrorKind::NotFound, "io", "not-found"),
        ] {
            let source = io::Error::new(kind, "private OS diagnostic must not escape");
            assert_eq!(io_kind(&source), delivery_kind);
            let error = output_projection_io_error(
                OutputStream::Stdout,
                ProjectionOperation::Write,
                "summary.json",
                source,
            );
            assert_eq!(error.code, crate::error::ErrorCode::OutputProjectionFailed);
            assert_eq!(error.message, "runtime output projection failed");
            assert_eq!(
                error.details,
                json!({"projections":[{
                    "stream":"stdout", "operation":"write", "kind":summary_kind,
                    "path":"summary.json", "bytesWritten":0
                }]})
            );
        }
    }

    #[test]
    fn delivery_diagnostic_keeps_intentional_lossy_path_and_safe_fields() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let error = projection_error(
            "live output delivery failed",
            vec![ProjectionIssue {
                stream: OutputStream::Stderr,
                operation: ProjectionOperation::Read,
                kind: "io".into(),
                path: PathBuf::from(OsString::from_vec(vec![b'/', 0xff])),
                bytes_written: 17,
            }],
        );
        assert_eq!(
            error.details,
            json!({"projections":[{
                "stream":"stderr","operation":"read","kind":"io","path":"/\u{fffd}","bytesWritten":17
            }]})
        );
    }
}
