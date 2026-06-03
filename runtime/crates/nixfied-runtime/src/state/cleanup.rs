use std::path::{Path, PathBuf};

use nixfied_model::CleanupPolicy;
use rusqlite::params;
use serde::Serialize;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::Registry;
use crate::state::marker::{StateIdentity, StateMarker, read_marker};
use crate::state::placement::canonicalize_existing;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOutcome {
    pub cleanup_id: String,
    pub deleted_path: PathBuf,
}

pub fn inspect_cleanup_target(
    state_base: impl AsRef<Path>,
    target: impl AsRef<Path>,
    expected: &StateIdentity,
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
    reject_tree_symlinks(&canonical_target)?;
    let marker = read_marker(&canonical_target)?;
    if !marker.matches_identity(expected) {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            "state marker identity does not match the requested cleanup identity",
        ));
    }
    if marker.cleanup_policy != CleanupPolicy::DeleteOnClean {
        return Err(RuntimeError::new(
            ErrorCode::CleanupRefused,
            "state cleanup policy requires explicit purge, which M0 does not implement",
        ));
    }
    Ok(marker)
}

pub fn clean_marked_state(
    state_base: impl AsRef<Path>,
    target: impl AsRef<Path>,
    expected: &StateIdentity,
    registry: &mut Registry,
) -> RuntimeResult<CleanupOutcome> {
    let target = target.as_ref();
    let marker = inspect_cleanup_target(state_base, target, expected)?;
    refuse_active_refs(registry)?;
    let canonical_target = canonicalize_existing("cleanup target", target)?;
    let cleanup_id = format!(
        "cleanup-{}-{}",
        std::process::id(),
        unix_time_nanos().unwrap_or(0)
    );
    let payload_json = cleanup_payload_json(&cleanup_id, &canonical_target);
    record_cleanup_intent(
        registry,
        &cleanup_id,
        &canonical_target,
        &marker,
        &payload_json,
    )?;
    if let Err(error) = std::fs::remove_dir_all(&canonical_target) {
        let cleanup_error = RuntimeError::new(
            ErrorCode::CleanupRefused,
            format!(
                "failed to delete cleanup target {}: {error}",
                canonical_target.display()
            ),
        );
        let _ = record_cleanup_terminal(
            registry,
            &cleanup_id,
            &marker.computed_model_hash,
            &payload_json,
            "failed",
            Some(cleanup_error.message.as_str()),
        );
        return Err(cleanup_error);
    }
    record_cleanup_terminal(
        registry,
        &cleanup_id,
        &marker.computed_model_hash,
        &payload_json,
        "deleted",
        None,
    )?;
    Ok(CleanupOutcome {
        cleanup_id,
        deleted_path: canonical_target,
    })
}

fn refuse_active_refs(registry: &Registry) -> RuntimeResult<()> {
    let active_lease_count = registry
        .connection()
        .query_row("SELECT count(*) FROM run_leases", [], |row| {
            row.get::<_, i64>(0)
        })
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
            "SELECT count(*) FROM processes WHERE status IN ('starting','running','ready')",
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
            "SELECT count(*) FROM ports WHERE status IN ('reserved','binding','bound','active')",
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
) -> RuntimeResult<()> {
    let marker_json = serde_json::to_string(marker).map_err(|error| {
        RuntimeError::new(
            ErrorCode::CleanupRefused,
            format!("failed to serialize cleanup marker: {error}"),
        )
    })?;
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT INTO cleanups (
              cleanup_id, target_path, marker_json, status, refusal_reason
            ) VALUES (?1, ?2, ?3, 'intent', NULL)
            ",
            params![cleanup_id, target.display().to_string(), marker_json],
        )
        .map_err(sql_error)?;
    insert_cleanup_event(
        &transaction,
        "cleanup.intent",
        &marker.computed_model_hash,
        payload_json,
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn record_cleanup_terminal(
    registry: &mut Registry,
    cleanup_id: &str,
    computed_model_hash: &str,
    payload_json: &str,
    status: &str,
    refusal_reason: Option<&str>,
) -> RuntimeResult<()> {
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE cleanups
            SET status = ?2, refusal_reason = ?3
            WHERE cleanup_id = ?1
            ",
            params![cleanup_id, status, refusal_reason],
        )
        .map_err(sql_error)?;
    let event_type = match status {
        "deleted" => "cleanup.deleted",
        "failed" => "cleanup.failed",
        _ => "cleanup.terminal",
    };
    insert_cleanup_event(&transaction, event_type, computed_model_hash, payload_json)?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn insert_cleanup_event(
    transaction: &rusqlite::Transaction<'_>,
    event_type: &str,
    computed_model_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    transaction
        .execute(
            "
            INSERT INTO events (
              at, event_type, run_id, service_instance_id, process_key,
              computed_model_hash, payload_json
            ) VALUES (
              strftime('%Y-%m-%dT%H:%M:%fZ','now'), ?1, NULL, NULL, NULL, ?2, ?3
            )
            ",
            params![event_type, computed_model_hash, payload_json],
        )
        .map_err(sql_error)?;
    Ok(())
}

fn cleanup_payload_json(cleanup_id: &str, target: &Path) -> String {
    serde_json::json!({
        "cleanupId": cleanup_id,
        "targetPath": target.display().to_string(),
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

fn reject_tree_symlinks(path: &Path) -> RuntimeResult<()> {
    for entry in std::fs::read_dir(path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "failed to inspect cleanup target {}: {error}",
                path.display()
            ),
        )
    })? {
        let entry = entry.map_err(|error| {
            RuntimeError::new(
                ErrorCode::StateUnowned,
                format!(
                    "failed to inspect cleanup target {}: {error}",
                    path.display()
                ),
            )
        })?;
        let entry_path = entry.path();
        let metadata = std::fs::symlink_metadata(&entry_path).map_err(|error| {
            RuntimeError::new(
                ErrorCode::StateUnowned,
                format!(
                    "failed to inspect cleanup target {}: {error}",
                    entry_path.display()
                ),
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(RuntimeError::new(
                ErrorCode::StateUnowned,
                format!("cleanup target contains symlink {}", entry_path.display()),
            ));
        }
        if metadata.is_dir() {
            reject_tree_symlinks(&entry_path)?;
        }
    }
    Ok(())
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
