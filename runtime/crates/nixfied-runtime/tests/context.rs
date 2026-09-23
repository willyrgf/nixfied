//! Native context precedence in child test processes, without mutating the
//! shared test process environment or substituting metadata for filesystem I/O.
mod common;

use common::TempDir;
use nixfied_runtime::{ErrorCode, admission::secrets::resolve_secrets, state::state_base_from_env};
use serde_json::json;
use std::{ffi::OsStr, fs, path::PathBuf, process::Command};

fn child(kind: &str, cwd: &std::path::Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "context_child", "--nocapture"])
        .env_clear()
        .env("NIXFIED_CONTEXT_CASE", kind)
        .current_dir(cwd);
    command
}

fn succeeds(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "context probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[test]
fn context_child() {
    let Ok(kind) = std::env::var("NIXFIED_CONTEXT_CASE") else {
        return;
    };
    if kind == "state" {
        let result = state_base_from_env();
        if let Some(expected) = std::env::var_os("EXPECTED_PATH") {
            assert_eq!(result.unwrap(), PathBuf::from(expected));
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::StateUnwritable);
        }
        return;
    }
    assert_eq!(kind, "secrets");
    let mut value = common::synthetic_manifest_default(23080, 23080);
    value["secrets"]["token"] = json!({"secretId":"token","source":{"kind":"file","path":"token"}});
    value["tasks"]["smoke"]["invocation"]["env"]["TOKEN"] = json!("${secret:token}");
    let manifest = serde_json::from_value(value).unwrap();
    let result = resolve_secrets(&manifest);
    if let Ok(expected) = std::env::var("EXPECTED_SECRET") {
        assert_eq!(result.unwrap().get("token"), Some(expected.as_str()));
    } else {
        assert_eq!(result.unwrap_err().code, ErrorCode::SecretUnavailable);
    }
}

#[test]
fn native_state_precedence_empty_values_and_lossless_paths() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let temp = TempDir::new();
    let home = temp.path.join("home");
    let xdg = temp.path.join("xdg");
    let explicit = temp.path.join("explicit");
    let suffix = if cfg!(target_os = "macos") {
        "Library/Application Support/nixfied"
    } else {
        ".local/state/nixfied"
    };
    for (override_value, xdg_value, expected) in [
        (
            Some(explicit.as_os_str()),
            Some(xdg.as_os_str()),
            explicit.clone(),
        ),
        (
            Some(OsStr::new("")),
            Some(xdg.as_os_str()),
            xdg.join("nixfied"),
        ),
        (None, Some(OsStr::new("")), home.join(suffix)),
        (None, None, home.join(suffix)),
    ] {
        let mut command = child("state", &temp.path);
        command.env("HOME", &home).env("EXPECTED_PATH", expected);
        if let Some(value) = override_value {
            command.env("NIXFIED_STATE_DIR", value);
        }
        if let Some(value) = xdg_value {
            command.env("XDG_STATE_HOME", value);
        }
        succeeds(&mut command);
    }
    succeeds(&mut child("state", &temp.path)); // No HOME or overrides rejects.
    succeeds(
        child("state", &temp.path)
            .env("HOME", "")
            .env("EXPECTED_PATH", suffix),
    );
    let bytes = OsString::from_vec(b"relative-\xff".to_vec());
    succeeds(
        child("state", &temp.path)
            .env("NIXFIED_STATE_DIR", &bytes)
            .env("EXPECTED_PATH", &bytes),
    );
    assert!(!explicit.exists()); // Looking up placement does not materialize it.
}

#[test]
fn native_secret_directory_precedence_reads_the_selected_material() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let temp = TempDir::new();
    let home = temp.path.join("home");
    let xdg = temp.path.join("xdg");
    let explicit = temp.path.join("explicit");
    let suffix = if cfg!(target_os = "macos") {
        "Library/Application Support/nixfied/secrets"
    } else {
        ".config/nixfied/secrets"
    };
    for (base, material) in [
        (&explicit, "explicit\r\n"),
        (&xdg.join("nixfied/secrets"), "xdg\n"),
        (&home.join(suffix), "home"),
    ] {
        fs::create_dir_all(base).unwrap();
        fs::write(base.join("token"), material).unwrap();
    }
    for (override_value, xdg_value, expected) in [
        (
            Some(explicit.as_os_str()),
            Some(xdg.as_os_str()),
            "explicit",
        ),
        (Some(OsStr::new("")), Some(xdg.as_os_str()), "xdg"),
        (None, Some(OsStr::new("")), "home"),
        (None, None, "home"),
    ] {
        let mut command = child("secrets", &temp.path);
        command.env("HOME", &home).env("EXPECTED_SECRET", expected);
        if let Some(value) = override_value {
            command.env("NIXFIED_SECRETS_DIR", value);
        }
        if let Some(value) = xdg_value {
            command.env("XDG_CONFIG_HOME", value);
        }
        succeeds(&mut command);
    }
    succeeds(&mut child("secrets", &temp.path));
    // A selected missing base rejects; it does not silently fall back to HOME.
    succeeds(
        child("secrets", &temp.path)
            .env("HOME", &home)
            .env("NIXFIED_SECRETS_DIR", temp.path.join("missing")),
    );
    let bytes = temp.path.join(OsString::from_vec(b"secrets-\xff".to_vec()));
    fs::create_dir_all(&bytes).unwrap();
    fs::write(bytes.join("token"), "bytes\r\n").unwrap();
    succeeds(
        child("secrets", &temp.path)
            .env("NIXFIED_SECRETS_DIR", bytes)
            .env("EXPECTED_SECRET", "bytes"),
    );
}
