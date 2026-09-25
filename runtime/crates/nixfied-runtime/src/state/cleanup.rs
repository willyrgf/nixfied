//! Crash-safe deletion of one marker-owned application-data generation.
//!
//! The slot owner commits one pending intent before any destructive effect,
//! deletes payload entries through directory descriptors without following
//! symlinks, keeps the root marker until every payload entry is gone, and then
//! commits completion. A pending intent is resumed by the same operation ID;
//! it is never replaced by a new attempt identity.
use crate::registry::sql_error;
use std::io;
use std::path::PathBuf;

use nixfied_manifest::PersistencePolicy;
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::filesystem::{Directory, EntryKind, FileIdentity};
use crate::registry::Registry;
use crate::registry::events::{EventInsert, insert_event};
use crate::registry::sqlite::RegistryContext;
use crate::registry::status::{CleanupStatus, DbStatus};
use crate::state::marker::{StateIdentity, StateMarker, refuse_unmarked_state_root};
use crate::state::tree::{ApplicationTree, MARKER, Observed, read_root_marker};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(
    tag = "result",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum CleanupOutcome {
    /// This operation deleted (or completed deleting) one data generation.
    Deleted {
        cleanup_id: String,
        deleted_path: PathBuf,
    },
    /// No application tree exists and no deletion is pending.
    Absent { target_path: PathBuf },
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

/// Delete the selected slot's application tree when its marker authorizes the
/// mode. A pending predecessor operation is resumed first under its own
/// committed authorization; the requested mode never extends it.
pub fn clean_marked_state(
    expected: &StateIdentity,
    registry: &mut Registry,
    mode: CleanupMode,
) -> RuntimeResult<CleanupOutcome> {
    let target = ApplicationTree::new(registry.authority(), expected)?;
    require_settled_slot(registry)?;
    if let Some(outcome) = resume_pending(registry, &target, expected)? {
        return Ok(outcome);
    }
    let Some(opened) = open_marked(&target, expected)? else {
        return Ok(CleanupOutcome::Absent {
            target_path: target.path,
        });
    };
    authorize(&opened.marker, mode)?;
    delete_generation(registry, &target, opened, mode)
}

/// What session settlement did with the application tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetentionOutcome {
    /// Persistent data survives the session.
    Retained,
    /// Run-scoped data was deleted after process quiescence.
    Deleted(CleanupOutcome),
    /// No application tree exists.
    Absent,
}

/// Apply the tree's own retention policy after process quiescence: run-scoped
/// data is deleted, persistent data is retained. The marker, not the current
/// manifest, authorizes deletion, so a changed manifest cannot make retained
/// persistent data disposable. Pending deletions are resumed first.
pub fn apply_retention(
    expected: &StateIdentity,
    registry: &mut Registry,
) -> RuntimeResult<RetentionOutcome> {
    let target = ApplicationTree::new(registry.authority(), expected)?;
    require_settled_slot(registry)?;
    if let Some(outcome) = resume_pending(registry, &target, expected)? {
        return Ok(RetentionOutcome::Deleted(outcome));
    }
    let Some(opened) = open_marked(&target, expected)? else {
        return Ok(RetentionOutcome::Absent);
    };
    opened.marker.check_abi(expected)?;
    match opened.marker.persistence {
        PersistencePolicy::Persistent => Ok(RetentionOutcome::Retained),
        PersistencePolicy::RunScoped => {
            delete_generation(registry, &target, opened, CleanupMode::Standard)
                .map(RetentionOutcome::Deleted)
        }
    }
}

/// A present application tree whose marker belongs to the expected owner.
struct OpenedTree {
    parent: Directory,
    root: Directory,
    identity: FileIdentity,
    marker: StateMarker,
}

fn open_marked(
    target: &ApplicationTree,
    expected: &StateIdentity,
) -> RuntimeResult<Option<OpenedTree>> {
    let (parent, root, identity) = match target.open()? {
        Observed::Absent => return Ok(None),
        Observed::Present {
            parent,
            root,
            identity,
        } => (parent, root, identity),
    };
    let Some(marker) = read_root_marker(&root)? else {
        // A fresh unmarked tree owns no data generation to retain or delete.
        refuse_unmarked_state_root(target, &root)?;
        return Ok(None);
    };
    if !marker.matches_ownership(expected) {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            "state marker identity does not match the requested cleanup identity",
        ));
    }
    Ok(Some(OpenedTree {
        parent,
        root,
        identity,
        marker,
    }))
}

/// Commit intent for an authorized generation, then delete it marker-last.
fn delete_generation(
    registry: &mut Registry,
    target: &ApplicationTree,
    opened: OpenedTree,
    mode: CleanupMode,
) -> RuntimeResult<CleanupOutcome> {
    refuse_deleted_generation(registry, &opened.marker)?;
    let record = CleanupRecord {
        cleanup_id: format!("cleanup-{}", crate::token::random_hex()?),
        target: target.relative.clone(),
        marker: opened.marker,
        purge: mode.is_purge(),
        root: opened.identity,
    };
    record_intent(registry, &record)?;
    finish_deletion(registry, target, &record, opened.parent, opened.root)
}

fn resume_pending(
    registry: &mut Registry,
    target: &ApplicationTree,
    expected: &StateIdentity,
) -> RuntimeResult<Option<CleanupOutcome>> {
    let Some(record) = pending_cleanup(registry)? else {
        return Ok(None);
    };
    if record.target != target.relative || !record.marker.matches_ownership(expected) {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            "pending cleanup does not belong to the selected application root",
        )
        .with_detail("cleanupId", &record.cleanup_id));
    }
    let outcome = match target.open()? {
        Observed::Absent => {
            target.sync_parent()?;
            complete(registry, &record)?;
            deleted(&record, target)
        }
        Observed::Present {
            parent,
            root,
            identity,
        } => {
            if identity != record.root {
                return Err(RuntimeError::new(
                    ErrorCode::StateUnowned,
                    "pending cleanup target was replaced; refusing to delete the replacement",
                )
                .with_detail("cleanupId", &record.cleanup_id));
            }
            match read_root_marker(&root)? {
                Some(marker) if marker == record.marker => {
                    finish_deletion(registry, target, &record, parent, root)?
                }
                Some(_) => {
                    return Err(RuntimeError::new(
                        ErrorCode::StateUnowned,
                        "pending cleanup target holds a different data generation",
                    )
                    .with_detail("cleanupId", &record.cleanup_id));
                }
                None => {
                    let names = root.entry_names().map_err(|error| target.io_error(error))?;
                    if !names.is_empty() {
                        return Err(RuntimeError::new(
                            ErrorCode::CleanupRefused,
                            "pending cleanup target lost its marker but is not empty; refusing to guess ownership",
                        )
                        .with_detail("cleanupId", &record.cleanup_id));
                    }
                    remove_root(target, &record, &parent, &root)
                        .map_err(|error| attempt_failed(registry, &record, error))?;
                    complete(registry, &record)?;
                    deleted(&record, target)
                }
            }
        }
    };
    Ok(Some(outcome))
}

/// Delete payload, then the marker, then the root, each step durable before the
/// next; commit completion last. Failure leaves the committed intent pending.
fn finish_deletion(
    registry: &mut Registry,
    target: &ApplicationTree,
    record: &CleanupRecord,
    parent: Directory,
    root: Directory,
) -> RuntimeResult<CleanupOutcome> {
    let removed = (|| {
        remove_contents(&root, record.root.device, true, 0)?;
        root.sync()?;
        let remaining = root.entry_names()?;
        if remaining.iter().any(|name| name.as_c_str() != MARKER) {
            return Err(io::Error::other(
                "unexpected entries appeared during cleanup",
            ));
        }
        root.remove_entry(MARKER, false)?;
        root.sync()?;
        remove_root(target, record, &parent, &root)
    })();
    removed.map_err(|error| attempt_failed(registry, record, error))?;
    complete(registry, record)?;
    Ok(deleted(record, target))
}

fn remove_root(
    target: &ApplicationTree,
    record: &CleanupRecord,
    parent: &Directory,
    root: &Directory,
) -> io::Result<()> {
    // Revalidate the held root against its entry before the final removal.
    match parent.entry(&target.name)? {
        Some(EntryKind::Directory(identity))
            if identity == record.root && root.identity()? == record.root => {}
        _ => return Err(io::Error::other("cleanup root entry changed")),
    }
    parent.remove_entry(&target.name, true)?;
    parent.sync()
}

/// Nesting bound for the descriptor-held traversal. Each level holds one
/// descriptor, so a deeper tree refuses before exhausting descriptors or stack.
const MAX_DEPTH: usize = 128;

fn remove_contents(
    directory: &Directory,
    device: u64,
    keep_marker: bool,
    depth: usize,
) -> io::Result<()> {
    for name in directory.entry_names()? {
        if keep_marker && name.as_c_str() == MARKER {
            continue;
        }
        match directory.entry(&name)? {
            None => {}
            Some(EntryKind::Directory(identity)) => {
                if identity.device != device || directory.is_mount_root(&name)? {
                    return Err(io::Error::other(
                        "cleanup refuses to traverse a nested mount",
                    ));
                }
                if depth >= MAX_DEPTH {
                    return Err(io::Error::other(
                        "cleanup refuses a tree deeper than its traversal bound",
                    ));
                }
                let child = directory.open_owned_child(&name)?;
                remove_contents(&child, device, false, depth + 1)?;
                drop(child);
                directory.remove_entry(&name, true)?;
            }
            Some(EntryKind::File(_) | EntryKind::Other(_)) => {
                directory.remove_entry(&name, false)?;
            }
        }
    }
    Ok(())
}

/// Persistence alone determines whether deletion is permitted. Purge overrides
/// retention only; ownership and process safety remain unconditional.
fn authorize(marker: &StateMarker, mode: CleanupMode) -> RuntimeResult<()> {
    match (&marker.persistence, mode) {
        (PersistencePolicy::RunScoped, _) | (PersistencePolicy::Persistent, CleanupMode::Purge) => {
            Ok(())
        }
        (PersistencePolicy::Persistent, CleanupMode::Standard) => Err(RuntimeError::new(
            ErrorCode::CleanupRefused,
            "persistent state requires explicit purge",
        )),
    }
}

fn deleted(record: &CleanupRecord, target: &ApplicationTree) -> CleanupOutcome {
    CleanupOutcome::Deleted {
        cleanup_id: record.cleanup_id.clone(),
        deleted_path: target.path.clone(),
    }
}

/// One committed deletion operation. The marker snapshot is the policy owner's
/// immutable authorization; it survives the marker's removal.
#[derive(Debug)]
struct CleanupRecord {
    cleanup_id: String,
    target: String,
    marker: StateMarker,
    purge: bool,
    root: FileIdentity,
}

/// The slot's one settled check: no recorded process obligation, and so no
/// endpoint evidence of an unsettled owner. Retention and cleanup refuse
/// before any deletion.
fn require_settled_slot(registry: &Registry) -> RuntimeResult<()> {
    registry.authority().validate()?;
    // Quiescence is the absence of recorded process obligations; endpoint
    // evidence settles with its owning process. Leader exit or a terminal
    // status alone settles nothing.
    let unresolved_process_count = registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE ownership = 'unresolved'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sql_error)?;
    if unresolved_process_count > 0 {
        return Err(RuntimeError::new(
            ErrorCode::CleanupRefused,
            "cleanup refused because unresolved process obligations exist",
        ));
    }
    Ok(())
}

fn pending_cleanup(registry: &Registry) -> RuntimeResult<Option<CleanupRecord>> {
    registry
        .connection()
        .query_row(
            "SELECT cleanup_id, target, data_generation, marker_json, purge, root_identity
             FROM cleanups WHERE status = ?1",
            [CleanupStatus::Pending.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()
        .map_err(sql_error)?
        .map(
            |(cleanup_id, target, generation, marker_json, purge, root)| {
                decode_record(cleanup_id, target, generation, &marker_json, purge, &root)
            },
        )
        .transpose()
}

/// Reject incoherent generation/authorization records before deletion.
fn decode_record(
    cleanup_id: String,
    target: String,
    generation: String,
    marker_json: &str,
    purge: i64,
    root: &str,
) -> RuntimeResult<CleanupRecord> {
    let corrupt = |message: &str| {
        RuntimeError::new(ErrorCode::RegistryCorrupt, message.to_owned())
            .with_detail("cleanupId", &cleanup_id)
    };
    let marker = serde_json::from_str::<StateMarker>(marker_json)
        .map_err(|_| corrupt("cleanup record has invalid marker evidence"))?;
    let purge = match purge {
        0 => false,
        1 => true,
        _ => return Err(corrupt("cleanup record has invalid authorization")),
    };
    if marker.data_generation != generation {
        return Err(corrupt(
            "cleanup record generation disagrees with its marker",
        ));
    }
    if !purge && marker.persistence != PersistencePolicy::RunScoped {
        return Err(corrupt(
            "cleanup record lacks authorization for persistent data",
        ));
    }
    let root = root
        .split_once(':')
        .and_then(|(device, inode)| {
            Some(FileIdentity {
                device: device.parse().ok()?,
                inode: inode.parse().ok()?,
            })
        })
        .ok_or_else(|| corrupt("cleanup record has invalid root identity"))?;
    Ok(CleanupRecord {
        cleanup_id,
        target,
        marker,
        purge,
        root,
    })
}

/// A generation that cleanup history already deleted can never be adopted as
/// application data again; its reappearance is contradictory history.
pub(crate) fn refuse_deleted_generation(
    registry: &Registry,
    marker: &StateMarker,
) -> RuntimeResult<()> {
    let deletions = registry
        .connection()
        .query_row(
            "SELECT count(*) FROM cleanups WHERE data_generation = ?1",
            [&marker.data_generation],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sql_error)?;
    if deletions > 0 {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            "a previously deleted data generation reappeared; refusing contradictory history",
        ));
    }
    Ok(())
}

fn payload(record: &CleanupRecord) -> String {
    serde_json::json!({
        "cleanupId": record.cleanup_id,
        "target": record.target,
        "dataGeneration": record.marker.data_generation,
        "purge": record.purge,
    })
    .to_string()
}

fn record_intent(registry: &mut Registry, record: &CleanupRecord) -> RuntimeResult<()> {
    let marker_json = serde_json::to_string(&record.marker).map_err(|error| {
        RuntimeError::new(
            ErrorCode::CleanupRefused,
            format!("failed to serialize cleanup marker: {error}"),
        )
    })?;
    let payload = payload(record);
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    transaction
        .execute(
            "INSERT INTO cleanups (
               cleanup_id, target, data_generation, marker_json, purge, root_identity, status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.cleanup_id,
                record.target,
                record.marker.data_generation,
                marker_json,
                i64::from(record.purge),
                record.root.to_string(),
                CleanupStatus::Pending.as_str(),
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        redactor,
        EventInsert::new("cleanup.intent", &payload),
    )?;
    transaction.commit().map_err(sql_error)
}

fn complete(registry: &mut Registry, record: &CleanupRecord) -> RuntimeResult<()> {
    let payload = payload(record);
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let changed = transaction
        .execute(
            "UPDATE cleanups SET status = ?2 WHERE cleanup_id = ?1 AND status = ?3",
            params![
                record.cleanup_id,
                CleanupStatus::Completed.as_str(),
                CleanupStatus::Pending.as_str(),
            ],
        )
        .map_err(sql_error)?;
    if changed != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "cleanup completion did not settle exactly one pending operation",
        ));
    }
    insert_event(
        &transaction,
        redactor,
        EventInsert::new("cleanup.completed", &payload),
    )?;
    transaction.commit().map_err(sql_error)
}

/// Keep the committed intent pending; attempt to record the safe failure.
fn attempt_failed(
    registry: &mut Registry,
    record: &CleanupRecord,
    error: io::Error,
) -> RuntimeError {
    let failure = RuntimeError::new(
        ErrorCode::CleanupRefused,
        format!(
            "cleanup {} remains pending after a failed deletion step: {error}",
            record.cleanup_id
        ),
    )
    .with_detail("cleanupId", &record.cleanup_id);
    let payload = serde_json::json!({
        "cleanupId": record.cleanup_id,
        "target": record.target,
        "reason": error.to_string(),
    })
    .to_string();
    match registry.append_event(EventInsert::new("cleanup.attempt-failed", &payload)) {
        Ok(_) => failure,
        Err(recording) => failure.with_cause(recording),
    }
}
