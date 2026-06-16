use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use nixfied_model::{CleanupPolicy, PersistencePolicy};
use nixfied_runtime::{
    Admission, AdmissionContext, ErrorCode, StoreOriginPolicy, load_model, read_raw_model,
};
use serde_json::{Value, json};

mod common;
use common::*;

fn fixture_model() -> Value {
    common::synthetic_model_default(23080, 23090)
}

#[test]
fn load_model_hashes_raw_bytes() {
    let (_tmp, model_path, _closure) = write_fixture_model(fixture_model(), true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let raw = fs::read(&model_path).expect("fixture should be readable");
    let expected = sha256_hex(&raw);

    assert_eq!(loaded.raw_len, raw.len());
    assert_eq!(loaded.computed_model_hash, expected);
}

#[test]
fn normal_admission_refuses_non_store_model() {
    let (_tmp, model_path, _closure) = write_fixture_model(fixture_model(), true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let error = Admission::check(
        &loaded,
        &AdmissionContext::current(StoreOriginPolicy::RequireStore),
    )
    .expect_err("non-store model must be refused");

    assert_eq!(error.code, ErrorCode::ModelNotStoreOutput);
    assert_eq!(
        error.computed_model_hash.as_deref(),
        Some(loaded.computed_model_hash.as_str())
    );
}

#[test]
fn normal_origin_refuses_non_store_before_parse() {
    let tmp = TempDir::new();
    let outside = tmp.path.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let model_path = outside.join("model.json");
    fs::write(&model_path, b"{not json").unwrap();
    let raw = read_raw_model(&model_path).expect("raw bytes should be readable");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::RequireStore,
        store_root: tmp.path.join("store"),
        host_system: host_system(),
    };
    fs::create_dir_all(&context.store_root).unwrap();
    let error = nixfied_runtime::admission::origin::check_raw_store_origin(&raw, &context)
        .expect_err("origin should gate before JSON parse");

    assert_eq!(error.code, ErrorCode::ModelNotStoreOutput);
}

#[test]
fn origin_rejects_canonical_path_escape() {
    let tmp = TempDir::new();
    let store = tmp.path.join("store");
    let outside = tmp.path.join("outside");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let real_model = outside.join("model.json");
    fs::write(&real_model, serde_json::to_vec(&fixture_model()).unwrap()).unwrap();
    let escaped_model = store.join("../outside/model.json");
    let raw = read_raw_model(&escaped_model).expect("escaped path should read");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::RequireStore,
        store_root: store,
        host_system: host_system(),
    };
    let error = nixfied_runtime::admission::origin::check_raw_store_origin(&raw, &context)
        .expect_err("canonical path outside store must fail");

    assert_eq!(error.code, ErrorCode::ModelNotStoreOutput);
}

#[test]
fn unstable_escape_hatch_admits_non_store_model() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root
            .parent()
            .expect("fixture closure has a parent")
            .to_path_buf(),
        host_system: host_system(),
    };
    let admission = Admission::check(&loaded, &context).expect("escape hatch should admit fixture");

    assert_eq!(admission.computed_model_hash, loaded.computed_model_hash);
    assert_eq!(admission.project_id, "runtime-test");
}

#[test]
fn source_admission_records_invocation_root() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let admission = Admission::check(&loaded, &context).expect("source should admit");
    let invocation_root = std::env::current_dir()
        .expect("current dir should exist")
        .canonicalize()
        .expect("current dir should canonicalize");

    let source = admission
        .require_source()
        .expect("run admission resolves a source");
    assert_eq!(source.codebase_id, "main");
    assert_eq!(source.logical_root, ".");
    assert_eq!(source.observed_root, invocation_root);
}

#[test]
fn lowering_failure_carries_model_provenance() {
    // A composite step referencing an undeclared task fails during lowering,
    // not parse/origin/abi/closure checks. That admission error must still
    // carry the model path and computed hash, like every other admission
    // failure.
    let mut model = fixture_model();
    model["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "steps": { "build": { "task": "missing-task" } }
    });
    let (_tmp, model_path, closure_root) = write_fixture_model(model, true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error =
        Admission::check(&loaded, &context).expect_err("undeclared step task must be refused");

    assert_eq!(error.code, ErrorCode::ModelAdmission);
    assert!(
        error.model_path.is_some(),
        "lowering error must carry a model path"
    );
    assert_eq!(
        error.computed_model_hash.as_deref(),
        Some(loaded.computed_model_hash.as_str())
    );
}

#[test]
fn control_admission_does_not_resolve_a_live_source() {
    // Recovery (ps/down/clean) must admit from the store model alone, so control
    // admission leaves the live workspace unresolved instead of failing when the
    // caller is outside the project root or the workspace has gone.
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let admission =
        Admission::check_for_control(&loaded, &context).expect("control admission should succeed");

    assert!(admission.source.is_none());
    assert_eq!(
        admission.require_source().unwrap_err().code,
        ErrorCode::SourceMismatch
    );
}

#[test]
fn dirty_policy_reject_fails_closed() {
    let mut model = fixture_model();
    model["codebases"][0]["sourcePolicy"]["dirtyPolicy"] = json!("reject");
    let (_tmp, model_path, closure_root) = write_fixture_model(model, true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context)
        .expect_err("dirtyPolicy=reject cannot be proven for live-workspace");

    assert_eq!(error.code, ErrorCode::SourceMismatch);
    assert_eq!(
        error.computed_model_hash.as_deref(),
        Some(loaded.computed_model_hash.as_str())
    );
}

#[test]
fn snapshot_source_admits_immutable_store_root_with_reject() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let store_root = closure_root.parent().unwrap().to_path_buf();
    let source_root = store_root.join("source-snapshot");
    fs::create_dir_all(source_root.join("app")).expect("immutable logical root should exist");
    let mut value: Value =
        serde_json::from_slice(&fs::read(&model_path).unwrap()).expect("fixture JSON");
    value["codebases"][0]["sourceMode"] = json!("snapshot");
    value["codebases"][0]["sourceIdentity"] = json!(source_root.to_string_lossy());
    value["codebases"][0]["logicalRoot"] = json!("app");
    value["codebases"][0]["sourcePolicy"]["dirtyPolicy"] = json!("reject");
    fs::write(&model_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root,
        host_system: host_system(),
    };

    let admission = Admission::check(&loaded, &context).expect("snapshot source should admit");
    let source = admission.require_source().expect("source should resolve");

    assert_eq!(source.source_mode, nixfied_model::SourceMode::Snapshot);
    assert_eq!(
        source.observed_root,
        source_root.join("app").canonicalize().unwrap()
    );
}

#[test]
fn flake_input_source_admits_immutable_store_root() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let store_root = closure_root.parent().unwrap().to_path_buf();
    let source_root = store_root.join("source-flake-input");
    fs::create_dir_all(&source_root).expect("immutable source root should exist");
    let mut value: Value =
        serde_json::from_slice(&fs::read(&model_path).unwrap()).expect("fixture JSON");
    value["codebases"][0]["sourceMode"] = json!("flake-input");
    value["codebases"][0]["sourceIdentity"] = json!(source_root.to_string_lossy());
    fs::write(&model_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root,
        host_system: host_system(),
    };

    let admission = Admission::check(&loaded, &context).expect("flake input source should admit");
    let source = admission.require_source().expect("source should resolve");

    assert_eq!(source.source_mode, nixfied_model::SourceMode::FlakeInput);
    assert_eq!(source.observed_root, source_root.canonicalize().unwrap());
}

#[test]
fn immutable_source_must_be_under_store_root() {
    let (tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let outside_source = tmp.path.join("outside-source");
    fs::create_dir_all(&outside_source).expect("outside source should exist");
    let mut value: Value =
        serde_json::from_slice(&fs::read(&model_path).unwrap()).expect("fixture JSON");
    value["codebases"][0]["sourceMode"] = json!("snapshot");
    value["codebases"][0]["sourceIdentity"] = json!(outside_source.to_string_lossy());
    fs::write(&model_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };

    let error = Admission::check(&loaded, &context)
        .expect_err("immutable source outside store root must fail");

    assert_eq!(error.code, ErrorCode::SourceMismatch);
}

#[test]
fn immutable_source_logical_root_escape_is_rejected() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let store_root = closure_root.parent().unwrap().to_path_buf();
    let source_root = store_root.join("source-snapshot-escape");
    fs::create_dir_all(&source_root).expect("immutable source root should exist");
    let mut value: Value =
        serde_json::from_slice(&fs::read(&model_path).unwrap()).expect("fixture JSON");
    value["codebases"][0]["sourceMode"] = json!("snapshot");
    value["codebases"][0]["sourceIdentity"] = json!(source_root.to_string_lossy());
    value["codebases"][0]["logicalRoot"] = json!("..");
    fs::write(&model_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root,
        host_system: host_system(),
    };

    let error =
        Admission::check(&loaded, &context).expect_err("immutable logicalRoot escape must fail");

    assert_eq!(error.code, ErrorCode::SourceMismatch);
}

#[test]
fn logical_root_escape_is_rejected() {
    let mut model = fixture_model();
    model["codebases"][0]["logicalRoot"] = json!("..");
    let (_tmp, model_path, closure_root) = write_fixture_model(model, true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context)
        .expect_err("logicalRoot must not escape invocation root");

    assert_eq!(error.code, ErrorCode::SourceMismatch);
}

#[test]
fn protected_persistent_state_is_valid_model_data() {
    let mut model = fixture_model();
    model["state"]["cleanupPolicy"] = json!("protected");
    model["state"]["persistence"] = json!("persistent");
    let (_tmp, model_path, closure_root) = write_fixture_model(model, true);
    let loaded = load_model(&model_path).expect("protected persistent state should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    Admission::check(&loaded, &context).expect("protected persistent state should admit");

    assert_eq!(loaded.model.state.cleanup_policy, CleanupPolicy::Protected);
    assert_eq!(
        loaded.model.state.persistence,
        PersistencePolicy::Persistent
    );
}

#[test]
fn abi_mismatch_is_runtime_abi_error() {
    let mut model = fixture_model();
    model["runtimeAbi"] = json!("nixfied-runtime-abi:legacy");
    let (_tmp, model_path, _closure) = write_fixture_model(model, true);
    let error = load_model(&model_path).expect_err("ABI mismatch should fail during load");

    assert_eq!(error.code, ErrorCode::RuntimeAbiMismatch);
    assert!(error.computed_model_hash.is_some());
}

#[test]
fn missing_closure_is_rejected() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), false);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root
            .parent()
            .expect("fixture closure has a parent")
            .to_path_buf(),
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context).expect_err("missing closure should fail");

    assert_eq!(error.code, ErrorCode::ClosureMissing);
}

#[test]
fn closure_store_path_escape_is_rejected() {
    let tmp = TempDir::new();
    let store = tmp.path.join("store");
    let outside_closure = tmp.path.join("outside-closure/test-synthetic-helper");
    let executable = outside_closure.join("bin/synthetic-helper");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(&executable).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&executable, perms).unwrap();

    let mut model = fixture_model();
    model["closures"]["synthetic-helper"]["storePath"] = json!(
        store
            .join("../outside-closure/test-synthetic-helper")
            .to_string_lossy()
    );
    model["closures"]["synthetic-helper"]["executable"] = json!(
        store
            .join("../outside-closure/test-synthetic-helper/bin/synthetic-helper")
            .to_string_lossy()
    );
    let escaped_executable = model["closures"]["synthetic-helper"]["executable"].clone();
    set_invocation_executables(&mut model, escaped_executable);
    let model_path = tmp.path.join("model.json");
    fs::write(&model_path, serde_json::to_vec(&model).unwrap()).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: store,
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context).expect_err("escaped closure path should fail");

    assert_eq!(error.code, ErrorCode::ClosureMissing);
}

#[test]
fn invocation_executable_must_match_run_resolution() {
    // A carried executable that disagrees with the declarative run[0]
    // resolution is rejected at lowering, fail closed.
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let mut value: Value =
        serde_json::from_slice(&fs::read(&model_path).unwrap()).expect("fixture JSON");
    value["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["executable"] =
        json!(closure_root.join("bin/other-helper").to_string_lossy());
    fs::write(&model_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context)
        .expect_err("invocation executable mismatch should fail");

    assert_eq!(error.code, ErrorCode::ModelAdmission);
    assert!(error.message.contains("resolve"));
}

#[test]
fn exec_bound_closure_without_executable_bit_is_rejected() {
    // requiresExecutable=false must not let an invoked closure skip the
    // executable-bit check: it is run via Command::new, so admission has to fail
    // closed (CLOSURE_MISSING) rather than defer to a runtime ProcEscape.
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let executable = closure_root.join("bin/synthetic-helper");
    let mut perms = fs::metadata(&executable).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&executable, perms).unwrap();
    let mut value: Value =
        serde_json::from_slice(&fs::read(&model_path).unwrap()).expect("fixture JSON");
    value["closures"]["synthetic-helper"]["requiresExecutable"] = json!(false);
    fs::write(&model_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context)
        .expect_err("non-executable exec-bound closure should fail");

    assert_eq!(error.code, ErrorCode::ClosureMissing);
}

#[test]
fn target_os_and_arch_must_match_host() {
    let mut model = fixture_model();
    model["target"]["os"] = json!("definitely-not-this-os");
    let (_tmp, model_path, closure_root) = write_fixture_model(model, true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context).expect_err("target OS mismatch should fail");

    assert_eq!(error.code, ErrorCode::PlatformUnsupported);
}

#[test]
fn symlinked_closure_executable_is_admitted() {
    // buildEnv-style packages symlink bin/<tool> into another store path of
    // the same closure (a Rust toolchain joins cargo/rustc components). The
    // declared executable lives under the declared storePath; following the
    // symlink must not break containment.
    let (tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    // Move the real binary outside the declared root and symlink it back in.
    let real_dir = tmp.path.join("store/real-component/bin");
    fs::create_dir_all(&real_dir).unwrap();
    let real = real_dir.join("synthetic-helper");
    let declared = closure_root.join("bin/synthetic-helper");
    fs::rename(&declared, &real).unwrap();
    std::os::unix::fs::symlink(&real, &declared).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    Admission::check(&loaded, &context).expect("symlinked executable should admit");
}

#[test]
fn symlinked_closure_executable_outside_store_root_is_rejected() {
    let (tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let outside_dir = tmp.path.join("outside/bin");
    fs::create_dir_all(&outside_dir).unwrap();
    let outside = outside_dir.join("synthetic-helper");
    let declared = closure_root.join("bin/synthetic-helper");
    fs::rename(&declared, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &declared).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };

    let error = Admission::check(&loaded, &context)
        .expect_err("symlink target outside the store root should fail");

    assert_eq!(error.code, ErrorCode::ClosureMissing);
    assert!(
        error.message.contains("closure.executable is not under"),
        "unexpected error message: {}",
        error.message
    );
}

fn write_fixture_model(mut value: Value, create_executable: bool) -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new();
    let closure_root = tmp.path.join("store/test-synthetic-helper");
    let executable = closure_root.join("bin/synthetic-helper");
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    value["closures"]["synthetic-helper"]["executable"] = json!(executable.to_string_lossy());
    set_invocation_executables(&mut value, json!(executable.to_string_lossy()));
    let model_path = tmp.path.join("model.json");
    if create_executable {
        fs::create_dir_all(executable.parent().expect("executable parent")).unwrap();
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        let mut perms = fs::metadata(&executable).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&executable, perms).unwrap();
    }
    fs::write(&model_path, serde_json::to_vec(&value).unwrap()).unwrap();
    (tmp, model_path, closure_root)
}

/// Point every fixture invocation's carried executable at the relocated
/// closure executable, keeping carried resolution and declaration coherent.
fn set_invocation_executables(value: &mut Value, executable: Value) {
    value["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["executable"] =
        executable.clone();
    value["tasks"]["smoke"]["invocation"]["executable"] = executable;
}

fn sha256_hex(raw: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    hex::encode(Sha256::digest(raw))
}
