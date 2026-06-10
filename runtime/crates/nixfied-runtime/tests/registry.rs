use nixfied_runtime::ErrorCode;
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
fn rejects_incompatible_registry_identity() {
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

    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
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

    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
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
