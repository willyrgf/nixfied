use nixfied_runtime::ErrorCode;
use nixfied_runtime::registry::leases::heartbeat_run_lease;
use nixfied_runtime::registry::{EventInsert, Registry, RegistryIdentity};

mod common;
use common::*;

#[test]
fn identity_diagnostics_preserve_every_field_and_negative_observed_slot() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let expected = RegistryIdentity::for_slot("project", "dev", 2, "abi", "tool");
    let registry = Registry::open_or_create(&path, &expected).unwrap();
    // Deliberately corrupt only this fixture; production writes keep the check.
    registry
        .connection()
        .execute_batch("PRAGMA ignore_check_constraints = ON;")
        .unwrap();
    registry
        .connection()
        .execute("UPDATE registry_meta SET slot = -7 WHERE id = 1", [])
        .unwrap();
    drop(registry);
    let error = match Registry::open_or_create(&path, &expected) {
        Ok(_) => panic!("negative observed slot must reject ownership"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert_eq!(
        error.details["expectedRegistryIdentity"],
        serde_json::json!({
            "projectId":"project","environment":"dev","slot":2,"runtimeAbi":"abi","toolchainId":"tool"
        })
    );
    assert_eq!(
        error.details["foundRegistryIdentity"],
        serde_json::json!({
            "projectId":"project","environment":"dev","slot":-7,"runtimeAbi":"abi","toolchainId":"tool"
        })
    );
    assert_eq!(
        error.details["mismatchedFields"],
        serde_json::json!(["slot"])
    );
    let original = error.details.clone();
    let cause = nixfied_runtime::error::RuntimeCause::from_error(error);
    assert_eq!(
        cause.details["foundRegistryIdentity"],
        original["foundRegistryIdentity"]
    );
    assert_eq!(
        cause.details["expectedRegistryIdentity"],
        original["expectedRegistryIdentity"]
    );
}

#[test]
fn concurrent_event_history_survives_reopen_and_rejects_another_slot() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry/registry.sqlite3");
    let identity = RegistryIdentity::for_slot("project", "staging", 3, "abi", "toolchain");
    let first = Registry::open_or_create(&path, &identity).unwrap();
    let second = Registry::open_or_create(&path, &identity).unwrap();
    let start = std::sync::Barrier::new(2);

    // Use the real write API on independently opened connections. Compare its
    // acknowledgements with persisted evidence after both connections close.
    let mut acknowledged = std::thread::scope(|scope| {
        let writers: Vec<_> = [first, second]
            .into_iter()
            .enumerate()
            .map(|(writer, mut registry)| {
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    (0..100)
                        .map(|index| {
                            let event_type = format!("writer-{writer}");
                            let payload = serde_json::json!({"index": index}).to_string();
                            let seq = registry
                                .append_event(&EventInsert::new(&event_type, &payload))
                                .unwrap();
                            (seq, event_type, payload)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        writers
            .into_iter()
            .flat_map(|writer| writer.join().unwrap())
            .collect::<Vec<_>>()
    });
    acknowledged.sort_by_key(|event| event.0);
    assert_eq!(
        acknowledged.iter().map(|event| event.0).collect::<Vec<_>>(),
        (1..=200).collect::<Vec<_>>()
    );

    let wrong_slot = RegistryIdentity::for_slot("project", "staging", 4, "abi", "toolchain");
    let error = match Registry::open_or_create(&path, &wrong_slot) {
        Ok(_) => panic!("another slot must not adopt the history"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::StateUnowned);

    let registry = Registry::open_or_create(&path, &identity).unwrap();
    let persisted = registry
        .connection()
        .prepare("SELECT seq, event_type, payload_json, environment, slot FROM events ORDER BY seq")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        persisted,
        acknowledged
            .into_iter()
            .map(|(seq, event_type, payload)| (seq, event_type, payload, "staging".into(), 3))
            .collect::<Vec<_>>()
    );
}

#[test]
fn all_clean_terminal_siblings_allow_heartbeat_shutdown() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = identity();
    let mut registry = Registry::open_or_create(&path, &identity).expect("registry should open");
    insert_run_for_heartbeat(&registry, "run-terminal");
    for (service, status) in [
        ("service-a", "completed"),
        ("service-b", "canceled"),
        ("service-c", "failed"),
    ] {
        registry
            .connection()
            .execute(
                "
                INSERT INTO run_leases (
                  run_id, environment, slot, service_instance_id, owner_token,
                  heartbeat_at, expires_at, status
                ) VALUES (?1, 'dev', 0, ?2, 'owner-token',
                          '2001-01-01T00:00:00.000Z', '2001-01-01T00:00:30.000Z', ?3)
                ",
                ("run-terminal", service, status),
            )
            .expect("lease should insert");
    }
    let before = heartbeat_rows(&registry, "run-terminal");

    heartbeat_run_lease(&mut registry, "run-terminal", "owner-token")
        .expect("a completely clean terminal run should stop heartbeat successfully");

    assert_eq!(heartbeat_rows(&registry, "run-terminal"), before);
}

#[test]
fn rejects_registry_project_mismatch_as_state_unowned() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = identity();
    Registry::open_or_create(&path, &identity).expect("registry should open");
    let bad = RegistryIdentity::default_slot(
        "other-project",
        "nixfied-runtime-abi:1",
        "nixfied-toolchain:1",
    );
    let error = match Registry::open_or_create(&path, &bad) {
        Ok(_) => panic!("identity mismatch should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert_mismatched_fields(&error, &["projectId"]);
    assert_registry_path_details(&error, &path);
}

#[test]
fn rejects_registry_environment_mismatch_as_state_unowned() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = identity();
    Registry::open_or_create(&path, &identity).expect("registry should open");
    let bad = RegistryIdentity::for_slot(
        "minimal",
        "prod",
        0,
        "nixfied-runtime-abi:1",
        "nixfied-toolchain:1",
    );
    let error = match Registry::open_or_create(&path, &bad) {
        Ok(_) => panic!("environment mismatch should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert_mismatched_fields(&error, &["environment"]);
    assert_registry_path_details(&error, &path);
}

#[test]
fn records_and_checks_selected_slot_identity() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = RegistryIdentity::for_slot(
        "minimal",
        "dev",
        1,
        "nixfied-runtime-abi:1",
        "nixfied-toolchain:1",
    );
    let registry = Registry::open_or_create(&path, &identity).expect("registry should open");
    let slot: i64 = registry
        .connection()
        .query_row("SELECT slot FROM registry_meta WHERE id = 1", [], |row| {
            row.get(0)
        })
        .expect("slot should be readable");
    assert_eq!(slot, 1);
    drop(registry);

    let wrong_slot =
        RegistryIdentity::default_slot("minimal", "nixfied-runtime-abi:1", "nixfied-toolchain:1");
    let error = match Registry::open_or_create(&path, &wrong_slot) {
        Ok(_) => panic!("slot mismatch should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert_mismatched_fields(&error, &["slot"]);
    assert_registry_path_details(&error, &path);
}

#[test]
fn rejects_registry_runtime_abi_mismatch_as_runtime_abi_mismatch() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = identity();
    Registry::open_or_create(&path, &identity).expect("registry should open");
    let bad =
        RegistryIdentity::default_slot("minimal", "nixfied-runtime-abi:2", "nixfied-toolchain:1");
    let error = match Registry::open_or_create(&path, &bad) {
        Ok(_) => panic!("runtime ABI mismatch should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::RuntimeAbiMismatch);
    assert_mismatched_fields(&error, &["runtimeAbi"]);
    assert_registry_path_details(&error, &path);
}

#[test]
fn rejects_registry_toolchain_mismatch_as_runtime_abi_mismatch() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = identity();
    Registry::open_or_create(&path, &identity).expect("registry should open");
    let bad =
        RegistryIdentity::default_slot("minimal", "nixfied-runtime-abi:1", "nixfied-toolchain:2");
    let error = match Registry::open_or_create(&path, &bad) {
        Ok(_) => panic!("toolchain mismatch should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::RuntimeAbiMismatch);
    assert_mismatched_fields(&error, &["toolchainId"]);
    assert_registry_path_details(&error, &path);
}

#[test]
fn rejects_incompatible_user_version() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = identity();
    let registry = Registry::open_or_create(&path, &identity).expect("registry should open");
    registry
        .connection()
        .execute_batch("PRAGMA user_version = 99;")
        .expect("test should mutate user_version");
    drop(registry);

    let error = match Registry::open_or_create(&path, &identity) {
        Ok(_) => panic!("schema mismatch should fail"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
}

#[test]
fn rejects_previous_service_status_schema_without_migration() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = identity();
    let registry = Registry::open_or_create(&path, &identity).expect("registry should open");
    registry
        .connection()
        .execute_batch("PRAGMA user_version = 5;")
        .expect("test should identify the removed service-status schema");
    drop(registry);

    let error = match Registry::open_or_create(&path, &identity) {
        Ok(_) => panic!("the prior registry shape must not be migrated"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
}

#[test]
fn rejects_previous_model_columns_without_rewriting_history() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA user_version = 6;
             CREATE TABLE events (seq INTEGER PRIMARY KEY, computed_model_hash TEXT);
             INSERT INTO events VALUES (1, 'historical-hash');",
        )
        .unwrap();
    }
    let error = match Registry::open_or_create(&path, &identity()) {
        Ok(_) => panic!("the previous schema must not be migrated"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let hash: String = conn
        .query_row(
            "SELECT computed_model_hash FROM events WHERE seq = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 6);
    assert_eq!(hash, "historical-hash");
}

#[test]
fn rejects_current_registry_missing_required_shape() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    {
        let conn = rusqlite::Connection::open(&path).expect("test DB should open");
        conn.pragma_update(
            None,
            "user_version",
            nixfied_runtime::registry::SCHEMA_VERSION,
        )
        .expect("test DB should use the current schema version");
        conn.execute_batch(
            "
            CREATE TABLE events (
              seq INTEGER PRIMARY KEY,
              at TEXT NOT NULL,
              event_type TEXT NOT NULL,
              run_id TEXT,
              service_instance_id TEXT,
              process_key TEXT,
              computed_manifest_hash TEXT,
              payload_json TEXT NOT NULL
            );
            ",
        )
        .expect("test DB should be initialized as corrupt current schema");
    }

    let error = match Registry::open_or_create(&path, &identity()) {
        Ok(_) => panic!("current registry without required shape should fail"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    assert_eq!(
        error.message,
        "registry table events is missing required column environment"
    );
}

#[test]
fn rejects_nonempty_unversioned_registry() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    {
        let conn = rusqlite::Connection::open(&path).expect("test DB should open");
        conn.execute_batch("CREATE TABLE unrelated (id INTEGER PRIMARY KEY);")
            .expect("test DB should contain unrelated state");
    }

    let error = match Registry::open_or_create(&path, &identity()) {
        Ok(_) => panic!("nonempty unversioned registry should fail"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
}

fn identity() -> RegistryIdentity {
    RegistryIdentity::default_slot("minimal", "nixfied-runtime-abi:1", "nixfied-toolchain:1")
}

fn insert_run_for_heartbeat(registry: &Registry, run_id: &str) {
    registry
        .connection()
        .execute(
            "
            INSERT INTO runs (
              run_id, environment, slot, status, manifest_path, computed_manifest_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            ) VALUES (?1, 'dev', 0, 'service-starting', '/nix/store/manifest.json',
                      'hash', 'nixfied-runtime-abi:1', 'nixfied-toolchain:1',
                      '{}', '{}', '{}', NULL)
            ",
            [run_id],
        )
        .expect("run should insert");
}

fn heartbeat_rows(registry: &Registry, run_id: &str) -> Vec<(String, String, String, String)> {
    registry
        .connection()
        .prepare(
            "
            SELECT service_instance_id, status, heartbeat_at, expires_at
            FROM run_leases
            WHERE run_id = ?1
            ORDER BY service_instance_id
            ",
        )
        .expect("lease rows should prepare")
        .query_map([run_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("lease rows should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("lease rows should collect")
}

fn assert_mismatched_fields(error: &nixfied_runtime::RuntimeError, expected: &[&str]) {
    let fields: Vec<&str> = error.details["mismatchedFields"]
        .as_array()
        .expect("mismatchedFields should be an array")
        .iter()
        .map(|field| field.as_str().expect("field should be a string"))
        .collect();
    assert_eq!(fields, expected);
    assert!(
        error.details["expectedRegistryIdentity"].is_object(),
        "expected identity should be recorded"
    );
    assert!(
        error.details["foundRegistryIdentity"].is_object(),
        "found identity should be recorded"
    );
}

fn assert_registry_path_details(error: &nixfied_runtime::RuntimeError, path: &std::path::Path) {
    assert_eq!(
        error.details["registryPath"].as_str(),
        Some(path.to_str().expect("registry path should be UTF-8"))
    );
    assert_eq!(
        error.details["registryDir"].as_str(),
        path.parent().and_then(std::path::Path::to_str).or(Some(""))
    );
}
