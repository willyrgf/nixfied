//! Native context precedence in child test processes, without mutating the
//! shared test process environment or substituting metadata for filesystem I/O.
mod common;

use common::{TempDir, assert_success};
use nixfied_runtime::{ErrorCode, admission::secrets::resolve_secrets, state::state_base_from_env};
use serde_json::json;
use std::{ffi::OsStr, fs, path::PathBuf, process::Command};

#[test]
fn new_run_rejects_non_utf8_state_or_source_metadata_before_slot_or_workload_effects() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    for bad_source in [false, true] {
        let temp = TempDir::new();
        let non_utf8 = temp.path.join(OsString::from_vec(b"native-\xff".to_vec()));
        let state = if bad_source {
            temp.path.join("state")
        } else {
            non_utf8.clone()
        };
        let counter = temp.path.join("workload-counter");
        let mut value = common::test_child_manifest(23080, 23080);
        value["tasks"]["smoke"]["requires"] = json!([]);
        let program = value["tasks"]["smoke"]["invocation"]["run"][0].clone();
        value["tasks"]["smoke"]["invocation"]["run"] =
            json!([program, "output", "occurrence", counter, "0"]);
        let manifest = temp.path.join("manifest.json");
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let cwd = if bad_source {
            fs::create_dir(&non_utf8).unwrap();
            non_utf8
        } else {
            temp.path.clone()
        };
        let output = Command::new(common::runtime_binary())
            .args(["run", "--allow-non-store-manifest", "--manifest"])
            .arg(&manifest)
            .args(["--task", "smoke", "--output", "json"])
            .env("NIXFIED_STATE_DIR", &state)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(31),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["code"], "LIFECYCLE_FAILED");
        assert!(output.stdout.is_empty());
        assert!(!state.exists(), "preflight must precede slot mutation");
        assert!(!counter.exists(), "preflight must precede workload release");
    }
}

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
    assert_success(&output);
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
    if kind == "absolute-placement-without-cwd" {
        let cwd = std::env::current_dir().unwrap();
        fs::remove_dir(cwd).unwrap();
        let base = PathBuf::from(std::env::var_os("EXPECTED_PATH").unwrap());
        let location =
            nixfied_runtime::state::derive_slot_location("project", "dev", 0, &base).unwrap();
        assert_eq!(location.state_base(), base);
        assert!(!base.exists());
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
fn absolute_placement_does_not_require_an_available_invocation_directory() {
    let temp = TempDir::new();
    let cwd = temp.path.join("removed-cwd");
    fs::create_dir(&cwd).unwrap();
    let mut command = child("absolute-placement-without-cwd", &cwd);
    command.env("EXPECTED_PATH", temp.path.join("state"));
    succeeds(&mut command);
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
    // macOS refuses to create a pathname containing invalid UTF-8 bytes.
    let selected = if cfg!(target_os = "macos") {
        temp.path.join("secrets-é")
    } else {
        temp.path.join(OsString::from_vec(b"secrets-\xff".to_vec()))
    };
    fs::create_dir_all(&selected).unwrap();
    fs::write(selected.join("token"), "bytes\r\n").unwrap();
    succeeds(
        child("secrets", &temp.path)
            .env("NIXFIED_SECRETS_DIR", selected)
            .env("EXPECTED_SECRET", "bytes"),
    );
}
