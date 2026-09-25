//! Process-loss schema proof runs in its own harness: fork-before-exec may
//! transiently inherit locks belonging to concurrent tests in another harness.
mod common;
use common::*;
use nixfied_runtime::registry::Registry;

#[test]
fn interrupted_creation_commits_neither_schema_nor_version() {
    let tmp = TempDir::new();
    let placement = registry_placement(&tmp.path, &registry_identity());
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
    Registry::open_or_create(registry_guard(&placement), &registry_identity())
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
    nixfied_runtime::registry::schema::initialize(&mut conn, &registry_identity()).unwrap();
    panic!("creation must reach the installed commit hook");
}
