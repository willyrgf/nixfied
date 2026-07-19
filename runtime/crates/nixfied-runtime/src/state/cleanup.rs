use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use nixfied_model::{CleanupPolicy, PersistencePolicy};
use rusqlite::params;
use serde::Serialize;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::Registry;
use crate::registry::events::{BorrowedEvent, insert_event};
use crate::registry::status::{self, CleanupStatus, DbStatus};
use crate::state::marker::{StateIdentity, StateMarker, read_marker};
use crate::state::placement::canonicalize_existing;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOutcome {
    pub cleanup_id: String,
    pub deleted_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupMode {
    Standard,
    Purge,
}

impl CleanupMode {
    pub fn is_purge(self) -> bool {
        matches!(self, Self::Purge)
    }
}

pub fn inspect_cleanup_target(
    state_base: impl AsRef<Path>,
    target: impl AsRef<Path>,
    expected: &StateIdentity,
    mode: CleanupMode,
) -> RuntimeResult<StateMarker> {
    let state_base = state_base.as_ref();
    let target = target.as_ref();
    let canonical_base = canonicalize_existing("state base", state_base)?;
    let canonical_target = canonicalize_existing("cleanup target", target)?;
    if !canonical_target.starts_with(&canonical_base) {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "cleanup target {} escapes state base {}",
                canonical_target.display(),
                canonical_base.display()
            ),
        ));
    }
    reject_target_symlink(target)?;
    let marker = read_marker(&canonical_target)?;
    // Cleanup is gated on ownership, not provenance: the slot's current owner
    // may clean a state root last used by an older build of the same model.
    if !marker.matches_ownership(expected) {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            "state marker identity does not match the requested cleanup identity",
        ));
    }
    refuse_cleanup_policy(&marker.cleanup_policy, &marker.persistence, mode)?;
    refuse_cleanup_policy(&expected.cleanup_policy, &expected.persistence, mode)?;
    Ok(marker)
}

pub fn clean_marked_state(
    state_base: impl AsRef<Path>,
    target: impl AsRef<Path>,
    expected: &StateIdentity,
    registry: &mut Registry,
    mode: CleanupMode,
) -> RuntimeResult<CleanupOutcome> {
    let target = target.as_ref();
    if target_is_missing(target)? {
        return finish_missing_target_cleanup(
            state_base.as_ref(),
            target,
            expected,
            registry,
            mode,
        );
    }
    let marker = inspect_cleanup_target(state_base, target, expected, mode)?;
    refuse_active_refs(registry)?;
    let canonical_target = canonicalize_existing("cleanup target", target)?;
    let cleanup_id = format!(
        "cleanup-{}-{}",
        std::process::id(),
        unix_time_nanos().unwrap_or(0)
    );
    let payload_json = cleanup_payload_json(&cleanup_id, &canonical_target, mode);
    record_cleanup_intent(
        registry,
        &cleanup_id,
        &canonical_target,
        &marker,
        &payload_json,
        mode,
    )?;
    if let Err(cleanup_error) = remove_dir_all_confined(&canonical_target, &canonical_target) {
        let _ = record_cleanup_terminal(
            registry,
            &cleanup_id,
            &marker.computed_model_hash,
            &payload_json,
            CleanupStatus::Failed,
            Some(cleanup_error.message.as_str()),
        );
        return Err(cleanup_error);
    }
    record_cleanup_terminal(
        registry,
        &cleanup_id,
        &marker.computed_model_hash,
        &payload_json,
        CleanupStatus::Deleted,
        None,
    )?;
    Ok(CleanupOutcome {
        cleanup_id,
        deleted_path: canonical_target,
    })
}

fn finish_missing_target_cleanup(
    state_base: &Path,
    target: &Path,
    expected: &StateIdentity,
    registry: &mut Registry,
    mode: CleanupMode,
) -> RuntimeResult<CleanupOutcome> {
    let canonical_target = canonicalize_missing_target(state_base, target)?;
    refuse_active_refs(registry)?;
    refuse_cleanup_policy(&expected.cleanup_policy, &expected.persistence, mode)?;
    let cleanup = find_prior_cleanup(registry, &canonical_target, expected)?;
    if CleanupStatus::from_db(&cleanup.status) == Some(CleanupStatus::Intent) {
        let payload_json =
            cleanup_payload_json(&cleanup.cleanup_id, &canonical_target, cleanup.mode);
        record_cleanup_terminal(
            registry,
            &cleanup.cleanup_id,
            &cleanup.marker.computed_model_hash,
            &payload_json,
            CleanupStatus::Deleted,
            None,
        )?;
    }
    Ok(CleanupOutcome {
        cleanup_id: cleanup.cleanup_id,
        deleted_path: canonical_target,
    })
}

#[derive(Debug)]
struct PriorCleanup {
    cleanup_id: String,
    status: String,
    mode: CleanupMode,
    marker: StateMarker,
}

fn target_is_missing(target: &Path) -> RuntimeResult<bool> {
    match std::fs::symlink_metadata(target) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "failed to inspect cleanup target {}: {error}",
                target.display()
            ),
        )),
    }
}

fn canonicalize_missing_target(state_base: &Path, target: &Path) -> RuntimeResult<PathBuf> {
    let canonical_base = canonicalize_existing("state base", state_base)?;
    let parent = target.parent().ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!("cleanup target {} has no parent", target.display()),
        )
    })?;
    let name = target.file_name().ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!("cleanup target {} has no final component", target.display()),
        )
    })?;
    let canonical_parent = canonicalize_existing("cleanup target parent", parent)?;
    if !canonical_parent.starts_with(&canonical_base) {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "cleanup target {} escapes state base {}",
                canonical_parent.join(name).display(),
                canonical_base.display()
            ),
        ));
    }
    Ok(canonical_parent.join(name))
}

fn find_prior_cleanup(
    registry: &Registry,
    canonical_target: &Path,
    expected: &StateIdentity,
) -> RuntimeResult<PriorCleanup> {
    let rows = {
        let mut statement = registry
            .connection()
            .prepare(&format!(
                "
                SELECT cleanup_id, marker_json, status, purge
                FROM cleanups
                WHERE target_path = ?1 AND status IN ({})
                ORDER BY rowid DESC
                ",
                status::sql_in_list(status::CLEANUP_PRIOR)
            ))
            .map_err(sql_error)?;
        statement
            .query_map([canonical_target.display().to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    for (cleanup_id, marker_json, status, purge) in rows {
        let marker = serde_json::from_str::<StateMarker>(&marker_json).map_err(|error| {
            RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!("cleanup {cleanup_id} has invalid marker evidence: {error}"),
            )
        })?;
        if marker.matches_ownership(expected) {
            return Ok(PriorCleanup {
                cleanup_id,
                status,
                mode: if purge == 0 {
                    CleanupMode::Standard
                } else {
                    CleanupMode::Purge
                },
                marker,
            });
        }
    }
    Err(RuntimeError::new(
        ErrorCode::StateUnowned,
        format!(
            "cleanup target {} is absent without matching cleanup evidence",
            canonical_target.display()
        ),
    ))
}

fn refuse_cleanup_policy(
    cleanup_policy: &CleanupPolicy,
    persistence: &PersistencePolicy,
    mode: CleanupMode,
) -> RuntimeResult<()> {
    if mode.is_purge() {
        return Ok(());
    }
    if cleanup_policy == &CleanupPolicy::DeleteOnClean
        && persistence == &PersistencePolicy::RunScoped
    {
        return Ok(());
    }
    Err(RuntimeError::new(
        ErrorCode::CleanupRefused,
        "state cleanup policy requires explicit purge",
    ))
}

fn refuse_active_refs(registry: &Registry) -> RuntimeResult<()> {
    let active_lease_count = registry
        .connection()
        .query_row(
            &format!(
                "SELECT count(*) FROM run_leases WHERE status IN ({})",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sql_error)?;
    if active_lease_count > 0 {
        return Err(RuntimeError::new(
            ErrorCode::CleanupRefused,
            "cleanup refused because active run leases exist",
        ));
    }

    let active_process_count = registry
        .connection()
        .query_row(
            &format!(
                "SELECT count(*) FROM processes WHERE status IN ({})",
                status::sql_in_list(status::PROCESS_ACTIVE)
            ),
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sql_error)?;
    if active_process_count > 0 {
        return Err(RuntimeError::new(
            ErrorCode::CleanupRefused,
            "cleanup refused because live process references exist",
        ));
    }

    let active_port_count = registry
        .connection()
        .query_row(
            &format!(
                "SELECT count(*) FROM ports WHERE status IN ({})",
                status::sql_in_list(status::PORT_OPEN)
            ),
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sql_error)?;
    if active_port_count > 0 {
        return Err(RuntimeError::new(
            ErrorCode::CleanupRefused,
            "cleanup refused because active port reservations exist",
        ));
    }
    Ok(())
}

fn record_cleanup_intent(
    registry: &mut Registry,
    cleanup_id: &str,
    target: &Path,
    marker: &StateMarker,
    payload_json: &str,
    mode: CleanupMode,
) -> RuntimeResult<()> {
    let marker_json = serde_json::to_string(marker).map_err(|error| {
        RuntimeError::new(
            ErrorCode::CleanupRefused,
            format!("failed to serialize cleanup marker: {error}"),
        )
    })?;
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT INTO cleanups (
              cleanup_id, environment, slot, target_path, purge, marker_json, status,
              refusal_reason
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
            ",
            params![
                cleanup_id,
                identity.environment.as_str(),
                identity.slot,
                target.display().to_string(),
                if mode.is_purge() { 1_i64 } else { 0_i64 },
                marker_json,
                CleanupStatus::Intent.as_str(),
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        &redactor,
        BorrowedEvent {
            event_type: "cleanup.intent",
            run_id: None,
            service_instance_id: None,
            process_key: None,
            computed_model_hash: Some(&marker.computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn record_cleanup_terminal(
    registry: &mut Registry,
    cleanup_id: &str,
    computed_model_hash: &str,
    payload_json: &str,
    status: CleanupStatus,
    refusal_reason: Option<&str>,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE cleanups
            SET status = ?2, refusal_reason = ?3
            WHERE cleanup_id = ?1
            ",
            params![cleanup_id, status.as_str(), refusal_reason],
        )
        .map_err(sql_error)?;
    let event_type = match status {
        CleanupStatus::Deleted => "cleanup.deleted",
        CleanupStatus::Failed => "cleanup.failed",
        CleanupStatus::Intent => "cleanup.terminal",
    };
    insert_event(
        &transaction,
        &identity,
        &redactor,
        BorrowedEvent {
            event_type,
            run_id: None,
            service_instance_id: None,
            process_key: None,
            computed_model_hash: Some(computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn cleanup_payload_json(cleanup_id: &str, target: &Path, mode: CleanupMode) -> String {
    serde_json::json!({
        "cleanupId": cleanup_id,
        "targetPath": target.display().to_string(),
        "purge": mode.is_purge(),
    })
    .to_string()
}

fn reject_target_symlink(target: &Path) -> RuntimeResult<()> {
    let metadata = std::fs::symlink_metadata(target).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "failed to inspect cleanup target {}: {error}",
                target.display()
            ),
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            format!("cleanup target is a symlink {}", target.display()),
        ));
    }
    Ok(())
}

fn remove_dir_all_confined(path: &Path, canonical_target: &Path) -> RuntimeResult<()> {
    for entry in std::fs::read_dir(path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::CleanupRefused,
            format!(
                "failed to inspect cleanup target {}: {error}",
                path.display()
            ),
        )
    })? {
        let entry = entry.map_err(|error| {
            RuntimeError::new(
                ErrorCode::CleanupRefused,
                format!(
                    "failed to inspect cleanup target {}: {error}",
                    path.display()
                ),
            )
        })?;
        let entry_path = entry.path();
        let metadata = std::fs::symlink_metadata(&entry_path).map_err(|error| {
            RuntimeError::new(
                ErrorCode::CleanupRefused,
                format!(
                    "failed to inspect cleanup target {}: {error}",
                    entry_path.display()
                ),
            )
        })?;
        if metadata.file_type().is_symlink() {
            std::fs::remove_file(&entry_path).map_err(|error| {
                RuntimeError::new(
                    ErrorCode::CleanupRefused,
                    format!(
                        "failed to unlink cleanup symlink {}: {error}",
                        entry_path.display()
                    ),
                )
            })?;
        } else if metadata.is_dir() {
            let canonical_entry = canonicalize_existing("cleanup entry", &entry_path)?;
            if !canonical_entry.starts_with(canonical_target) {
                return Err(RuntimeError::new(
                    ErrorCode::StateUnowned,
                    format!(
                        "cleanup entry {} escapes cleanup target {}",
                        canonical_entry.display(),
                        canonical_target.display()
                    ),
                ));
            }
            remove_dir_all_confined(&entry_path, canonical_target)?;
        } else {
            std::fs::remove_file(&entry_path).map_err(|error| {
                RuntimeError::new(
                    ErrorCode::CleanupRefused,
                    format!(
                        "failed to delete cleanup entry {}: {error}",
                        entry_path.display()
                    ),
                )
            })?;
        }
    }
    std::fs::remove_dir(path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::CleanupRefused,
            format!("failed to delete cleanup dir {}: {error}", path.display()),
        )
    })
}

fn unix_time_nanos() -> Option<u128> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos())
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string())
}
