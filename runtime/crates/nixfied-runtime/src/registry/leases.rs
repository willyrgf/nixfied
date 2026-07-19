use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{TransactionBehavior, params};

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
    let transaction = registry
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let statuses = {
        let mut statement = transaction
            .prepare("SELECT status FROM run_leases WHERE run_id = ?1 AND owner_token = ?2")
            .map_err(sql_error)?;
        statement
            .query_map(params![run_id, owner_token], |row| row.get::<_, String>(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    if statuses.is_empty() {
        return Err(stale_lease_error(run_id));
    }
    let parsed = statuses
        .iter()
        .map(|status| {
            RunLeaseStatus::from_db(status).ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    format!("run lease {run_id} has unknown status {status}"),
                )
            })
        })
        .collect::<RuntimeResult<Vec<_>>>()?;
    let open_count = parsed
        .iter()
        .filter(|status| status::LEASE_OPEN.contains(status))
        .count();
    if open_count == 0 {
        let clean_terminal = parsed.iter().all(|status| {
            matches!(
                status,
                RunLeaseStatus::Completed | RunLeaseStatus::Canceled | RunLeaseStatus::Failed
            )
        });
        if clean_terminal {
            transaction.commit().map_err(sql_error)?;
            return Ok(());
        }
        return Err(stale_lease_error(run_id));
    }
    let ttl_modifier = lease_ttl_modifier();
    let updated = transaction
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
    if updated != open_count {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!(
                "run lease heartbeat updated {updated} rows for {run_id}, expected {open_count}"
            ),
        ));
    }
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn stale_lease_error(run_id: &str) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::LeaseStale,
        format!("run lease {run_id} is no longer active for this owner"),
    )
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
    RuntimeError::new(
        ErrorCode::RegistryCorrupt,
        format!("run lease registry operation failed: {error}"),
    )
}
