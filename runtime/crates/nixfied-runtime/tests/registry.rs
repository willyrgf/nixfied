use nixfied_runtime::ErrorCode;
use nixfied_runtime::registry::leases::heartbeat_run_lease;
use nixfied_runtime::registry::{EventInsert, Registry, RegistryIdentity, SCHEMA_VERSION};

mod common;
use common::*;

#[test]
fn creates_registry_schema_with_wal() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry/registry.sqlite3");
    let identity = identity();
    let registry = Registry::open_or_create(&path, &identity).expect("registry should open");
    let journal_mode: String = registry
        .connection()
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("journal mode should be readable");
    let user_version: i64 = registry
        .connection()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user version should be readable");
    let table_count: i64 = registry
        .connection()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('registry_meta','events','runs','services','processes','ports','run_leases','cleanups')",
            [],
            |row| row.get(0),
        )
        .expect("table count should be readable");

    assert_eq!(registry.path(), path.as_path());
    assert_eq!(journal_mode, "wal");
    assert_eq!(user_version, SCHEMA_VERSION);
    assert_eq!(table_count, 8);
}

#[test]
fn open_sets_a_busy_timeout() {
    // The run's heartbeat thread opens a second connection and writes the lease
    // concurrently with the main thread; a non-zero busy timeout makes a writer
    // collision wait rather than return SQLITE_BUSY (mapped to REGISTRY_CORRUPT).
    let tmp = TempDir::new();
    let path = tmp.path.join("registry/registry.sqlite3");
    let identity = identity();
    let registry = Registry::open_or_create(&path, &identity).expect("registry should open");
    let busy_timeout: i64 = registry
        .connection()
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .expect("busy_timeout should be readable");
    assert_eq!(busy_timeout, 5000);
    drop(registry);

    // A reopened (existing) registry — the heartbeat's second-connection path —
    // passes through the same `initialize`, so it carries the timeout too.
    let reopened = Registry::open_or_create(&path, &identity).expect("registry should reopen");
    let reopened_timeout: i64 = reopened
        .connection()
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .expect("busy_timeout should be readable");
    assert_eq!(reopened_timeout, 5000);
}

#[test]
fn concurrent_writers_do_not_corrupt() {
    // Two connections to the same registry writing at once must not surface
    // SQLITE_BUSY as REGISTRY_CORRUPT — the busy timeout absorbs the contention.
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = identity();
    Registry::open_or_create(&path, &identity).expect("registry should open");

    const WRITES: usize = 200;
    let other_path = path.clone();
    let other_identity = identity.clone();
    let writer = std::thread::spawn(move || {
        let mut registry =
            Registry::open_or_create(&other_path, &other_identity).expect("second handle opens");
        for _ in 0..WRITES {
            registry
                .append_event(&EventInsert::new("writer-b", "{}"))
                .expect("concurrent append must not fail");
        }
    });

    let mut registry = Registry::open_or_create(&path, &identity).expect("first handle opens");
    for _ in 0..WRITES {
        registry
            .append_event(&EventInsert::new("writer-a", "{}"))
            .expect("concurrent append must not fail");
    }
    writer.join().expect("writer thread should not panic");

    let count: i64 = registry
        .connection()
        .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
        .expect("event count should be readable");
    assert_eq!(count, (WRITES * 2) as i64);
}

#[test]
fn appends_events_with_total_ordering() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    let identity = identity();
    let mut registry = Registry::open_or_create(&path, &identity).expect("registry should open");
    let first = registry
        .append_event(&EventInsert::new("first", "{}"))
        .expect("first event should append");
    let second = registry
        .append_event(&EventInsert::new("second", "{}"))
        .expect("second event should append");
    let scopes = registry
        .connection()
        .prepare("SELECT environment, slot FROM events ORDER BY seq")
        .expect("events should prepare")
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .expect("events should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("events should collect");

    assert_eq!(first + 1, second);
    assert_eq!(scopes, [("dev".to_string(), 0), ("dev".to_string(), 0)]);
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
fn rejects_existing_v1_registry_missing_required_shape() {
    let tmp = TempDir::new();
    let path = tmp.path.join("registry.sqlite3");
    {
        let conn = rusqlite::Connection::open(&path).expect("test DB should open");
        conn.execute_batch(
            "
            PRAGMA user_version = 1;
            CREATE TABLE events (
              seq INTEGER PRIMARY KEY,
              at TEXT NOT NULL,
              event_type TEXT NOT NULL,
              run_id TEXT,
              service_instance_id TEXT,
              process_key TEXT,
              computed_model_hash TEXT,
              payload_json TEXT NOT NULL
            );
            ",
        )
        .expect("test DB should be initialized as corrupt v1");
    }

    let error = match Registry::open_or_create(&path, &identity()) {
        Ok(_) => panic!("existing v1 registry without required shape should fail"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
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
              run_id, environment, slot, status, model_path, computed_model_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            ) VALUES (?1, 'dev', 0, 'service-starting', '/nix/store/model.json',
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
