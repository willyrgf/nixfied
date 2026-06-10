use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{OptionalExtension, params};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::status::{self, DbStatus, RunLeaseStatus};
use crate::registry::{Registry, RegistryIdentity};

pub const RUN_LEASE_HEARTBEAT_SECS: u64 = 5;
pub const RUN_LEASE_TTL_SECS: u64 = 30;

pub fn heartbeat_run_lease(
    registry: &mut Registry,
    run_id: &str,
    owner_token: &str,
) -> RuntimeResult<()> {
    let ttl_modifier = lease_ttl_modifier();
    let updated = registry
        .connection_mut()
        .execute(
            &format!(
                "
            UPDATE run_leases
            SET heartbeat_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                expires_at = strftime('%Y-%m-%dT%H:%M:%fZ','now', ?3)
            WHERE run_id = ?1 AND owner_token = ?2 AND status IN ({})
            ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![run_id, owner_token, ttl_modifier],
        )
        .map_err(sql_error)?;
    if updated != 1 {
        if lease_is_terminal_for_owner(registry, run_id, owner_token)? {
            return Ok(());
        }
        return Err(RuntimeError::new(
            ErrorCode::LeaseStale,
            format!("run lease {run_id} is no longer active for this owner"),
        ));
    }
    Ok(())
}

fn lease_is_terminal_for_owner(
    registry: &Registry,
    run_id: &str,
    owner_token: &str,
) -> RuntimeResult<bool> {
    let status = registry
        .connection()
        .query_row(
            "SELECT status FROM run_leases WHERE run_id = ?1 AND owner_token = ?2",
            params![run_id, owner_token],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_error)?;
    Ok(status
        .as_deref()
        .and_then(RunLeaseStatus::from_db)
        .is_some_and(|status| status::LEASE_TERMINAL.contains(&status)))
}

pub fn lease_ttl_modifier() -> String {
    format!("+{RUN_LEASE_TTL_SECS} seconds")
}

pub struct RunLeaseHeartbeat {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<RuntimeResult<()>>>,
}

impl RunLeaseHeartbeat {
    pub fn start(
        registry_path: PathBuf,
        identity: RegistryIdentity,
        run_id: String,
        owner_token: String,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut registry = Registry::open_or_create(registry_path, &identity)?;
            while !thread_stop.load(Ordering::SeqCst) {
                heartbeat_run_lease(&mut registry, &run_id, &owner_token)?;
                sleep_until_next_heartbeat(&thread_stop);
            }
            Ok(())
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }

    pub fn stop(mut self) -> RuntimeResult<()> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap_or_else(|_| {
                Err(RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    "run lease heartbeat thread panicked",
                ))
            })?;
        }
        Ok(())
    }
}

impl Drop for RunLeaseHeartbeat {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn sleep_until_next_heartbeat(stop: &AtomicBool) {
    let deadline = Instant::now() + Duration::from_secs(RUN_LEASE_HEARTBEAT_SECS);
    while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string())
}
