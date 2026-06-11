//! Shared fixtures and helpers for the integration-test binaries. Each `tests/*.rs`
//! pulls this in with `mod common;`; no single binary uses every item, so dead
//! code is expected here rather than a sign of rot.
#![allow(dead_code)]
#![allow(unused_imports)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use nixfied_model::fixtures::{self, SyntheticModelOptions};
pub use nixfied_model::fixtures::{SYNTHETIC_EXECUTABLE, SYNTHETIC_START_ARGS};

/// The canonical admission fixture — a `synthetic` foreground service plus a
/// `smoke` task in slot 0 over the given candidate port window. Delegates to
/// `nixfied_model::fixtures`, the single source of truth for the model shape.
pub fn synthetic_model(
    executable: &str,
    start_args: &[&str],
    port_start: u16,
    port_end: u16,
) -> Value {
    fixtures::synthetic_model(&SyntheticModelOptions {
        executable: executable.to_string(),
        start_args: start_args.iter().map(|s| s.to_string()).collect(),
        port_start,
        port_end,
        ..SyntheticModelOptions::default()
    })
}

/// [`synthetic_model`] with the default executable and start arguments.
pub fn synthetic_model_default(port_start: u16, port_end: u16) -> Value {
    synthetic_model(
        SYNTHETIC_EXECUTABLE,
        SYNTHETIC_START_ARGS,
        port_start,
        port_end,
    )
}

pub use nixfied_model::fixtures::{host_arch, host_os, host_system};

/// A unique temporary directory removed on drop.
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "nixfied-test-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// The trailing JSON document a runtime command writes to stderr, after any
/// human-readable progress lines (which never contain `{`).
pub fn stderr_json(bytes: &[u8]) -> Value {
    let text = String::from_utf8_lossy(bytes);
    let start = text
        .find('{')
        .expect("stderr should contain a JSON document");
    serde_json::from_str(&text[start..]).expect("stderr JSON should parse")
}

/// A unique temporary path (not created) with the given prefix.
pub fn temp_marker(prefix: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    path
}

pub fn unique_suffix() -> u128 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be available")
        .as_nanos();
    now + u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}
