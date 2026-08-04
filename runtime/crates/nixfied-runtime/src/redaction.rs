use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::path::Path;
use std::process::Stdio;
use std::thread::{self, JoinHandle};

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
        if let Some(hash) = &mut error.computed_model_hash {
            *hash = self.redact_text(hash);
        }
        error
    }

    fn redact_bytes(&self, input: &[u8]) -> Vec<u8> {
        if self.is_empty() {
            return input.to_vec();
        }
        let mut out = Vec::with_capacity(input.len());
        let mut index = 0;
        while index < input.len() {
            if let Some(pattern) = self
                .patterns
                .iter()
                .find(|pattern| input[index..].starts_with(pattern.as_slice()))
            {
                out.extend_from_slice(REDACTION_TOKEN.as_bytes());
                index += pattern.len();
            } else {
                out.push(input[index]);
                index += 1;
            }
        }
        out
    }

    fn redact_available(&self, pending: &mut Vec<u8>) -> Vec<u8> {
        let keep = self.max_pattern_len().saturating_sub(1);
        let process_len = pending.len().saturating_sub(keep);
        let mut out = Vec::with_capacity(process_len);
        let mut index = 0;
        while index < process_len {
            if let Some(pattern) = self
                .patterns
                .iter()
                .find(|pattern| pending[index..].starts_with(pattern.as_slice()))
            {
                out.extend_from_slice(REDACTION_TOKEN.as_bytes());
                index += pattern.len();
            } else {
                out.push(pending[index]);
                index += 1;
            }
        }
        pending.drain(..index);
        out
    }

    fn max_pattern_len(&self) -> usize {
        self.patterns
            .first()
            .map(|pattern| pattern.len())
            .unwrap_or(0)
    }
}

pub struct RedactedLogRelays {
    handles: Vec<JoinHandle<RuntimeResult<()>>>,
}

impl RedactedLogRelays {
    pub fn empty() -> Self {
        Self {
            handles: Vec::new(),
        }
    }

    pub fn join(self) -> RuntimeResult<()> {
        for handle in self.handles {
            match handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(RuntimeError::new(
                        ErrorCode::SecretLeakBlocked,
                        "redaction relay panicked before proving captured output was scrubbed",
                    ));
                }
            }
        }
        Ok(())
    }
}

pub struct RedactedChildOutput {
    pub stdout: Stdio,
    pub stderr: Stdio,
    pub relays: RedactedLogRelays,
}

pub fn child_output(
    stdout_path: &Path,
    stderr_path: &Path,
    redactor: &Redactor,
) -> RuntimeResult<RedactedChildOutput> {
    if redactor.is_empty() {
        return Ok(RedactedChildOutput {
            stdout: Stdio::from(create_log_file(stdout_path, false)?),
            stderr: Stdio::from(create_log_file(stderr_path, false)?),
            relays: RedactedLogRelays::empty(),
        });
    }

    let (stdout, stdout_relay) = redacted_stdio(stdout_path, redactor)?;
    let (stderr, stderr_relay) = redacted_stdio(stderr_path, redactor)?;
    Ok(RedactedChildOutput {
        stdout,
        stderr,
        relays: RedactedLogRelays {
            handles: vec![stdout_relay, stderr_relay],
        },
    })
}

fn redacted_stdio(
    path: &Path,
    redactor: &Redactor,
) -> RuntimeResult<(Stdio, JoinHandle<RuntimeResult<()>>)> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(leak_blocked(format!(
            "failed to create redaction pipe for {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    let read_end = unsafe { File::from_raw_fd(fds[0]) };
    let write_end = unsafe { File::from_raw_fd(fds[1]) };
    let writer = create_log_file(path, true)?;
    let redactor = redactor.clone();
    let handle = thread::spawn(move || redact_stream(read_end, writer, redactor));
    Ok((Stdio::from(write_end), handle))
}

fn redact_stream(mut reader: File, mut writer: File, redactor: Redactor) -> RuntimeResult<()> {
    let mut pending = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let read = reader.read(&mut buf).map_err(|error| {
            leak_blocked(format!(
                "failed to read child output for redaction: {error}"
            ))
        })?;
        if read == 0 {
            break;
        }
        pending.extend_from_slice(&buf[..read]);
        if pending.len() >= redactor.max_pattern_len() {
            let redacted = redactor.redact_available(&mut pending);
            writer.write_all(&redacted).map_err(|error| {
                leak_blocked(format!("failed to write redacted child output: {error}"))
            })?;
        }
    }
    if !pending.is_empty() {
        let redacted = redactor.redact_bytes(&pending);
        writer.write_all(&redacted).map_err(|error| {
            leak_blocked(format!(
                "failed to write final redacted child output: {error}"
            ))
        })?;
    }
    writer
        .flush()
        .map_err(|error| leak_blocked(format!("failed to flush redacted child output: {error}")))?;
    Ok(())
}

fn create_log_file(path: &Path, redacted: bool) -> RuntimeResult<File> {
    File::create(path).map_err(|error| {
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
