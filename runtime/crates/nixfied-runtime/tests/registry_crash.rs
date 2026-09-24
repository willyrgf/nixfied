//! Process-loss schema proof runs in its own harness: fork-before-exec may
//! transiently inherit locks belonging to concurrent tests in another harness.
mod common;
use common::*;
use nixfied_runtime::registry::{Registry, RegistryIdentity};

#[test]
fn interrupted_creation_commits_neither_schema_nor_version() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &identity());
    let path = placement.registry_path();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "registry_creation_crash_child", "--nocapture"])
        .env("NIXFIED_TEST_REGISTRY_CRASH_PATH", &path)
        .status()
        .unwrap();
    assert_eq!(
        status.code(),
        Some(73),
        "child must exit at the SQLite commit boundary"
    );
    assert_empty_unversioned_database(&path);
    Registry::open_or_create(registry_guard(&placement), &identity())
        .expect("interrupted creation must be retryable");
}

#[test]
fn registry_creation_crash_child() {
    let Some(path) = std::env::var_os("NIXFIED_TEST_REGISTRY_CRASH_PATH") else {
        return;
    };
    let mut conn = rusqlite::Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    unsafe extern "C" fn exit_before_commit(_: *mut std::ffi::c_void) -> std::ffi::c_int {
        // Do not unwind or run Connection/Transaction destructors: model process loss.
        unsafe { libc::_exit(73) }
    }
    // SAFETY: the connection outlives the callback and the callback reads no data.
    unsafe {
        rusqlite::ffi::sqlite3_commit_hook(
            conn.handle(),
            Some(exit_before_commit),
            std::ptr::null_mut(),
        );
    }
    nixfied_runtime::registry::schema::initialize(&mut conn, &identity()).unwrap();
    panic!("creation must reach the installed commit hook");
}

fn identity() -> RegistryIdentity {
    RegistryIdentity::default_slot("minimal", "nixfied-runtime-abi:1", "nixfied-toolchain:1")
}
fn registry_placement(
    root: &std::path::Path,
    identity: &RegistryIdentity,
) -> nixfied_runtime::state::HostPlacement {
    let placement = nixfied_runtime::state::placement::derive_slot_placement(
        &identity.project_id,
        &identity.environment,
        identity.slot.try_into().unwrap(),
        "crash-test",
        root,
    )
    .unwrap();
    registry_guard(&placement).release().unwrap();
    placement
}
fn assert_empty_unversioned_database(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
