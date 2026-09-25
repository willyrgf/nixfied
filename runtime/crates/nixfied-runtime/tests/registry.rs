use nixfied_runtime::ErrorCode;
use nixfied_runtime::registry::{EventInsert, Registry, RegistryIdentity, RegistryReader};

mod common;
use common::*;

#[test]
fn competing_initializers_admit_one_writer_before_schema_creation() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let start = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let creators: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    let guard = nixfied_runtime::state::ownership::SlotGuard::acquire(
                        &placement,
                        &nixfied_runtime::cancellation::CancellationToken::new(),
                    )?;
                    Registry::open_or_create(guard, &registry_identity())
                })
            })
            .collect();
        creators
            .into_iter()
            .map(|creator| creator.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .next()
            .unwrap()
            .code,
        ErrorCode::CleanupRefused
    );
    let winner = results
        .iter()
        .find_map(|result| result.as_ref().ok())
        .unwrap();
    assert_eq!(
        winner
            .connection()
            .query_row("SELECT count(*) FROM registry_meta", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        winner
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        nixfied_runtime::registry::SCHEMA_VERSION
    );
    let tables = winner
        .connection()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        tables,
        [
            "cleanups",
            "events",
            "ports",
            "processes",
            "registry_meta",
            "runs"
        ]
    );
    assert!(!placement.state_root().exists());
    assert!(!placement.run_dir().exists());
    drop(results);
    Registry::open_or_create(registry_guard(&placement), &registry_identity())
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn invalid_identity_is_rejected_before_database_creation() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let path = placement.registry_path();
    let mut invalid = registry_identity();
    invalid.slot = -1;
    let error = Registry::open_or_create(registry_guard(&placement), &invalid)
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert!(!path.exists());
    Registry::open_or_create(registry_guard(&placement), &registry_identity())
        .expect("rejected admission must leave the slot usable");
}

#[test]
fn failed_version_write_rolls_back_tables_and_metadata() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let path = placement.registry_path();
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    unsafe extern "C" fn deny_version_write(
        _: *mut std::ffi::c_void,
        action: std::ffi::c_int,
        name: *const std::ffi::c_char,
        value: *const std::ffi::c_char,
        _: *const std::ffi::c_char,
        _: *const std::ffi::c_char,
    ) -> std::ffi::c_int {
        if action == rusqlite::ffi::SQLITE_PRAGMA && !name.is_null() && !value.is_null()
            // SAFETY: SQLite supplies a NUL-terminated pragma name for this call.
            && unsafe { std::ffi::CStr::from_ptr(name) } == c"user_version"
        {
            rusqlite::ffi::SQLITE_DENY
        } else {
            rusqlite::ffi::SQLITE_OK
        }
    }
    // SAFETY: no callback state is borrowed, and the connection owns its lifetime.
    assert_eq!(
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(
                conn.handle(),
                Some(deny_version_write),
                std::ptr::null_mut(),
            )
        },
        rusqlite::ffi::SQLITE_OK
    );
    let error =
        nixfied_runtime::registry::schema::initialize(&mut conn, &registry_identity()).unwrap_err();
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    drop(conn);
    assert_empty_unversioned_database(&path);
    Registry::open_or_create(registry_guard(&placement), &registry_identity())
        .expect("failed version write must be retryable");
}

#[test]
fn identity_diagnostics_preserve_every_field_and_negative_observed_slot() {
    let tmp = TempDir::new();
    let expected = RegistryIdentity::for_slot("project", "dev", 2, "abi", "tool");
    let placement = registry_placement(&tmp.path, &expected);
    let registry = Registry::open_or_create(registry_guard(&placement), &expected).unwrap();
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
    let error = Registry::open_or_create(registry_guard(&placement), &expected)
        .err()
        .expect("negative observed slot must reject ownership");
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
fn event_history_survives_successive_owners_and_rejects_another_slot() {
    let tmp = TempDir::new();
    let identity = RegistryIdentity::for_slot("project", "staging", 3, "abi", "toolchain");
    let placement = registry_placement(&tmp.path, &identity);
    let mut acknowledged = Vec::new();
    for writer in 0..2 {
        let mut registry = Registry::open_or_create(registry_guard(&placement), &identity).unwrap();
        for index in 0..100 {
            let event_type = format!("writer-{writer}");
            let payload = serde_json::json!({"index": index}).to_string();
            let seq = registry
                .append_event(EventInsert::new(&event_type, &payload))
                .unwrap();
            acknowledged.push((seq, event_type, payload, "staging".to_string(), 3_i64));
        }
        registry.close().unwrap();
    }
    assert_eq!(
        acknowledged.iter().map(|event| event.0).collect::<Vec<_>>(),
        (1..=200).collect::<Vec<_>>()
    );
    let wrong = RegistryIdentity::for_slot("project", "staging", 4, "abi", "toolchain");
    assert!(
        matches!(Registry::open_or_create(registry_guard(&placement), &wrong), Err(error) if error.code == ErrorCode::StateUnowned)
    );
    let registry = Registry::open_or_create(registry_guard(&placement), &identity).unwrap();
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
    assert_eq!(persisted, acknowledged);
}

#[test]
fn registry_identity_mismatches_preserve_classification_and_stored_registry_identity() {
    for (field, bad, code) in [
        (
            "projectId",
            RegistryIdentity {
                project_id: "other-project".into(),
                ..registry_identity()
            },
            ErrorCode::StateUnowned,
        ),
        (
            "environment",
            RegistryIdentity {
                environment: "prod".into(),
                ..registry_identity()
            },
            ErrorCode::StateUnowned,
        ),
        (
            "runtimeAbi",
            RegistryIdentity {
                runtime_abi: "nixfied-runtime-abi:2".into(),
                ..registry_identity()
            },
            ErrorCode::RuntimeAbiMismatch,
        ),
        (
            "toolchainId",
            RegistryIdentity {
                toolchain_id: "nixfied-toolchain:2".into(),
                ..registry_identity()
            },
            ErrorCode::RuntimeAbiMismatch,
        ),
    ] {
        let tmp = TempDir::new();
        let placement = registry_placement(&tmp.path, &registry_identity());
        let path = placement.registry_path();
        let expected = registry_identity();
        Registry::open_or_create(registry_guard(&placement), &expected)
            .expect("registry should open");
        let error = RegistryReader::open_existing(&path, &bad)
            .err()
            .unwrap_or_else(|| panic!("{field} mismatch should fail"));
        assert_eq!(error.code, code, "{field}");
        assert_mismatched_fields(&error, &[field]);
        assert_registry_path_details(&error, &path);
        Registry::open_or_create(registry_guard(&placement), &expected)
            .expect("rejection must preserve the original stored identity");
    }
}

#[test]
fn records_and_checks_selected_slot_registry_identity() {
    let tmp = TempDir::new();
    let identity = RegistryIdentity::for_slot(
        "minimal",
        "dev",
        1,
        "nixfied-runtime-abi:1",
        "nixfied-toolchain:1",
    );
    let placement = registry_placement(&tmp.path, &identity);
    let path = placement.registry_path();
    let registry = Registry::open_or_create(registry_guard(&placement), &identity)
        .expect("registry should open");
    let slot: i64 = registry
        .connection()
        .query_row("SELECT slot FROM registry_meta WHERE id = 1", [], |row| {
            row.get(0)
        })
        .expect("slot should be readable");
    assert_eq!(slot, 1);
    drop(registry);

    let wrong_slot = RegistryIdentity::for_slot(
        "minimal",
        "dev",
        0,
        "nixfied-runtime-abi:1",
        "nixfied-toolchain:1",
    );
    let error = RegistryReader::open_existing(&path, &wrong_slot)
        .err()
        .expect("slot mismatch should fail");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert_mismatched_fields(&error, &["slot"]);
    assert_registry_path_details(&error, &path);
}

#[test]
fn rejects_current_registry_missing_required_shape() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let path = placement.registry_path();
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

    let error = Registry::open_or_create(registry_guard(&placement), &registry_identity())
        .err()
        .expect("current registry without required shape should fail");
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    assert_eq!(
        error.message,
        "registry schema does not match its exact version"
    );
}

#[test]
fn rejects_nonempty_unversioned_registry() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let path = placement.registry_path();
    {
        let conn = rusqlite::Connection::open(&path).expect("test DB should open");
        conn.execute_batch("CREATE TABLE unrelated (id INTEGER PRIMARY KEY);")
            .expect("test DB should contain unrelated state");
    }

    let error = Registry::open_or_create(registry_guard(&placement), &registry_identity())
        .err()
        .expect("nonempty unversioned registry should fail");
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
}

#[test]
fn exact_schema_and_identity_reject_before_journal_conversion() {
    for mutation in [
        "ALTER TABLE events RENAME COLUMN payload_json TO lost_payload",
        "ALTER TABLE events ADD COLUMN unexpected TEXT",
        "CREATE TABLE unexpected (value TEXT)",
        "CREATE TRIGGER suppress_event BEFORE INSERT ON events BEGIN SELECT RAISE(IGNORE); END",
        "CREATE INDEX unexpected_index ON events(event_type)",
        "PRAGMA writable_schema = ON; UPDATE sqlite_schema SET sql = replace(sql, 'CHECK (slot >= 0)', '') WHERE name = 'events'; PRAGMA writable_schema = OFF",
        "PRAGMA user_version = 99",
        "UPDATE registry_meta SET toolchain_id = 'other-toolchain'",
    ] {
        let tmp = TempDir::new();
        let placement = registry_placement(&tmp.path, &registry_identity());
        let path = placement.registry_path();
        Registry::open_or_create(registry_guard(&placement), &registry_identity())
            .unwrap()
            .close()
            .unwrap();
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA journal_mode = DELETE").unwrap();
            conn.execute_batch(mutation).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let expected = if mutation.contains("other-toolchain") {
            ErrorCode::RuntimeAbiMismatch
        } else {
            ErrorCode::RegistryCorrupt
        };
        let reader_error = RegistryReader::open_existing(&path, &registry_identity())
            .err()
            .expect(mutation);
        assert_eq!(reader_error.code, expected, "{mutation}");
        let writer_error =
            Registry::open_or_create(registry_guard(&placement), &registry_identity())
                .err()
                .expect(mutation);
        assert_eq!(writer_error.code, expected, "{mutation}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "{mutation}");
        assert!(!path.with_extension("sqlite3-wal").exists(), "{mutation}");
        assert!(!path.with_extension("sqlite3-shm").exists(), "{mutation}");
        let conn = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "delete",
            "{mutation}"
        );
    }
}

#[test]
fn writer_requires_verified_durability_on_creation_and_reopen() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    for _ in 0..2 {
        let registry =
            Registry::open_or_create(registry_guard(&placement), &registry_identity()).unwrap();
        let conn = registry.connection();
        assert_eq!(
            conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        assert_eq!(
            conn.query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        #[cfg(target_os = "macos")]
        for setting in ["fullfsync", "checkpoint_fullfsync"] {
            assert_eq!(
                conn.query_row(&format!("PRAGMA {setting}"), [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
        registry.close().unwrap();
    }
    // SQLite silently keeps journal_mode=memory for this connection. A setting
    // request that succeeds is insufficient; the boundary must verify its result.
    let mut memory = rusqlite::Connection::open_in_memory().unwrap();
    let error = nixfied_runtime::registry::schema::initialize(&mut memory, &registry_identity())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    assert_eq!(
        memory
            .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn ignored_synchronous_setting_refuses_before_schema_creation() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let path = placement.registry_path();
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA synchronous = OFF").unwrap();
    unsafe extern "C" fn ignore_sync_write(
        _: *mut std::ffi::c_void,
        action: std::ffi::c_int,
        name: *const std::ffi::c_char,
        value: *const std::ffi::c_char,
        _: *const std::ffi::c_char,
        _: *const std::ffi::c_char,
    ) -> std::ffi::c_int {
        if action == rusqlite::ffi::SQLITE_PRAGMA && !name.is_null() && !value.is_null()
            // SAFETY: SQLite supplies this NUL-terminated name during callback.
            && unsafe { std::ffi::CStr::from_ptr(name) } == c"synchronous"
        {
            rusqlite::ffi::SQLITE_IGNORE
        } else {
            rusqlite::ffi::SQLITE_OK
        }
    }
    // SAFETY: callback has no borrowed state and lives for the connection.
    assert_eq!(
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(
                conn.handle(),
                Some(ignore_sync_write),
                std::ptr::null_mut(),
            )
        },
        rusqlite::ffi::SQLITE_OK
    );
    let error =
        nixfied_runtime::registry::schema::initialize(&mut conn, &registry_identity()).unwrap_err();
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    drop(conn);
    assert_empty_unversioned_database(&path);
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

fn session_state(registry: &Registry, run_id: &str) -> (Option<String>, String) {
    registry
        .connection()
        .query_row(
            "SELECT execution_outcome, finalization FROM runs WHERE run_id = ?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

#[test]
fn session_outcome_is_immutable_and_does_not_claim_resource_finalization() {
    use nixfied_runtime::registry::session::{ExecutionOutcome, record_execution_outcome};
    for (outcome, wire) in [
        (ExecutionOutcome::Succeeded, "succeeded"),
        (ExecutionOutcome::Failed, "failed"),
        (ExecutionOutcome::Canceled, "canceled"),
    ] {
        let tmp = TempDir::new();
        let placement = registry_placement(&tmp.path, &registry_identity());
        let mut registry =
            Registry::open_or_create(registry_guard(&placement), &registry_identity()).unwrap();
        seed_run(registry.connection(), "run", None);
        assert_eq!(
            record_execution_outcome(&mut registry, "run", "hash", ExecutionOutcome::Interrupted)
                .unwrap_err()
                .code,
            ErrorCode::RegistryCorrupt
        );
        assert_eq!(session_state(&registry, "run"), (None, "unfinished".into()));
        for _ in 0..2 {
            record_execution_outcome(&mut registry, "run", "hash", outcome).unwrap();
        }
        let different = if outcome == ExecutionOutcome::Succeeded {
            ExecutionOutcome::Failed
        } else {
            ExecutionOutcome::Succeeded
        };
        assert_eq!(
            record_execution_outcome(&mut registry, "run", "hash", different)
                .unwrap_err()
                .code,
            ErrorCode::RegistryCorrupt
        );
        assert_eq!(
            session_state(&registry, "run"),
            (Some(wire.into()), "unfinished".into())
        );
        let events: i64 = registry
            .connection()
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 1);
    }
}

#[test]
fn failed_outcome_event_rolls_back_the_execution_result() {
    use nixfied_runtime::registry::session::{ExecutionOutcome, record_execution_outcome};
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let mut registry =
        Registry::open_or_create(registry_guard(&placement), &registry_identity()).unwrap();
    seed_run(registry.connection(), "run", None);
    registry.connection().execute_batch("CREATE TRIGGER reject_outcome BEFORE INSERT ON events
        WHEN NEW.event_type = 'run.execution-settled' BEGIN SELECT RAISE(ABORT, 'outcome-denied'); END;").unwrap();
    let error = record_execution_outcome(&mut registry, "run", "hash", ExecutionOutcome::Succeeded)
        .unwrap_err();
    assert!(error.message.contains("outcome-denied"));
    assert_eq!(session_state(&registry, "run"), (None, "unfinished".into()));
}

#[test]
fn successor_interrupts_only_unknown_execution_and_preserves_known_outcomes() {
    use nixfied_runtime::registry::session::{
        ExecutionOutcome, record_execution_outcome, record_interrupted_sessions,
    };
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let mut registry =
        Registry::open_or_create(registry_guard(&placement), &registry_identity()).unwrap();
    for run_id in ["unknown", "succeeded", "failed"] {
        seed_run(registry.connection(), run_id, None);
    }
    record_execution_outcome(
        &mut registry,
        "succeeded",
        "hash",
        ExecutionOutcome::Succeeded,
    )
    .unwrap();
    record_execution_outcome(&mut registry, "failed", "hash", ExecutionOutcome::Failed).unwrap();
    registry.close().unwrap();
    let mut successor =
        Registry::open_or_create(registry_guard(&placement), &registry_identity()).unwrap();
    for _ in 0..2 {
        record_interrupted_sessions(&mut successor).unwrap();
    }
    for (run, outcome) in [
        ("unknown", "interrupted"),
        ("succeeded", "succeeded"),
        ("failed", "failed"),
    ] {
        assert_eq!(
            session_state(&successor, run),
            (Some(outcome.into()), "unfinished".into())
        );
    }
    let interruptions: i64 = successor
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'run.interrupted'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(interruptions, 1);
}

#[test]
fn recovery_validates_all_session_records_before_mutating_any() {
    use nixfied_runtime::registry::session::record_interrupted_sessions;
    for mutation in [
        "environment = 'other'",
        "slot = 1",
        "execution_outcome = 'unknown'",
        "finalization = 'unknown'",
        "finalization = 'complete'",
    ] {
        let tmp = TempDir::new();
        let placement = registry_placement(&tmp.path, &registry_identity());
        let mut registry =
            Registry::open_or_create(registry_guard(&placement), &registry_identity()).unwrap();
        for run in ["a-pending", "z-damaged"] {
            seed_run(registry.connection(), run, None);
        }
        // Model damaged stored bytes outside the supported writer boundary.
        registry
            .connection()
            .execute_batch("PRAGMA ignore_check_constraints = ON")
            .unwrap();
        registry
            .connection()
            .execute(
                &format!("UPDATE runs SET {mutation} WHERE run_id = 'z-damaged'"),
                [],
            )
            .unwrap();
        assert_eq!(
            record_interrupted_sessions(&mut registry).unwrap_err().code,
            ErrorCode::RegistryCorrupt
        );
        assert_eq!(
            session_state(&registry, "a-pending"),
            (None, "unfinished".into())
        );
        let events: i64 = registry
            .connection()
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 0, "{mutation}");
    }
}

#[test]
fn schema_rejects_finalization_without_execution_outcome() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let registry =
        Registry::open_or_create(registry_guard(&placement), &registry_identity()).unwrap();
    seed_run(registry.connection(), "run", None);
    assert!(
        registry
            .connection()
            .execute("UPDATE runs SET finalization = 'complete'", [])
            .is_err()
    );
    assert_eq!(session_state(&registry, "run"), (None, "unfinished".into()));
}

#[test]
fn ignored_outcome_updates_cannot_publish_false_settlement_events() {
    use nixfied_runtime::registry::session::{
        ExecutionOutcome, record_execution_outcome, record_interrupted_sessions,
    };
    for recovery in [false, true] {
        let tmp = TempDir::new();
        let placement = registry_placement(&tmp.path, &registry_identity());
        let mut registry =
            Registry::open_or_create(registry_guard(&placement), &registry_identity()).unwrap();
        for run in ["a-pending", "z-pending"] {
            seed_run(registry.connection(), run, None);
        }
        registry
            .connection()
            .execute_batch(
                "CREATE TRIGGER ignore_outcome BEFORE UPDATE ON runs
            WHEN OLD.run_id = 'z-pending' BEGIN SELECT RAISE(IGNORE); END;",
            )
            .unwrap();
        let result = if recovery {
            record_interrupted_sessions(&mut registry)
        } else {
            record_execution_outcome(
                &mut registry,
                "z-pending",
                "hash",
                ExecutionOutcome::Succeeded,
            )
        };
        assert_eq!(result.unwrap_err().code, ErrorCode::RegistryCorrupt);
        for run in ["a-pending", "z-pending"] {
            assert_eq!(session_state(&registry, run), (None, "unfinished".into()));
        }
        let events: i64 = registry
            .connection()
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 0);
    }
}

#[test]
fn output_seals_only_after_closed_registration_and_settled_capture_and_never_on_a_failed_commit() {
    use nixfied_runtime::registry::session::{
        OutputPublication, close_source_registration, seal_output,
    };
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
    let mut registry =
        Registry::open_or_create(registry_guard(&placement), &registry_identity()).unwrap();
    seed_run(registry.connection(), "run", None);
    let output = |registry: &Registry| -> (String, i64) {
        registry
            .connection()
            .query_row(
                "SELECT output, (SELECT count(*) FROM events WHERE event_type = 'run.output-sealed')
                 FROM runs WHERE run_id = 'run'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    };

    // Open registration never seals.
    assert_eq!(
        seal_output(&mut registry, "run", "hash").unwrap_err().code,
        ErrorCode::RegistryCorrupt
    );
    seed_process(
        registry.connection(),
        SeedProcess {
            key: "task",
            run_id: "run",
            pid: 1,
            status: "exited",
            ownership: "settled",
            presentation: "shown",
            ..SeedProcess::default()
        },
    );
    close_source_registration(&mut registry, "run", "hash").unwrap();

    // A source without a checked capture outcome leaves completeness unknown.
    assert!(matches!(
        seal_output(&mut registry, "run", "hash").unwrap(),
        OutputPublication::Unsealed
    ));
    assert_eq!(output(&registry), ("unsealed".into(), 0));
    registry
        .connection()
        .execute("UPDATE processes SET capture = 'complete'", [])
        .unwrap();

    // A failed seal commit publishes no seal.
    registry
        .connection()
        .execute_batch(
            "CREATE TRIGGER reject_seal BEFORE INSERT ON events
             WHEN NEW.event_type = 'run.output-sealed' BEGIN SELECT RAISE(ABORT, 'seal-denied'); END;",
        )
        .unwrap();
    let error = seal_output(&mut registry, "run", "hash").unwrap_err();
    assert!(error.message.contains("seal-denied"), "{error:?}");
    assert_eq!(output(&registry), ("unsealed".into(), 0));

    registry
        .connection()
        .execute_batch("DROP TRIGGER reject_seal;")
        .unwrap();
    for _ in 0..2 {
        assert!(matches!(
            seal_output(&mut registry, "run", "hash").unwrap(),
            OutputPublication::Sealed
        ));
    }
    assert_eq!(output(&registry), ("sealed".into(), 1));
}
