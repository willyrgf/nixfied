use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde_json::{Value, json};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::records::RegistryIdentity;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistryIdentityDiagnostic<'a> {
    project_id: &'a str,
    environment: &'a str,
    slot: i64,
    runtime_abi: &'a str,
    toolchain_id: &'a str,
}

pub const SCHEMA_VERSION: i64 = 13;

pub fn initialize(conn: &mut Connection, identity: &RegistryIdentity) -> RuntimeResult<()> {
    conn.execute_batch(
        "
        PRAGMA busy_timeout = 5000;
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        ",
    )
    .map_err(sql_error)?;
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let starting_user_version = transaction
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(sql_error)?;
    if starting_user_version != 0 && starting_user_version != SCHEMA_VERSION {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("expected registry user_version {SCHEMA_VERSION}, got {starting_user_version}"),
        ));
    }
    let sqlite_table_count = user_table_count(&transaction)?;
    let is_new_registry = starting_user_version == 0 && sqlite_table_count == 0;
    if !is_new_registry {
        if starting_user_version != SCHEMA_VERSION {
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!(
                    "expected registry user_version {SCHEMA_VERSION}, got {starting_user_version}"
                ),
            ));
        }
        verify_required_columns(&transaction)?;
        verify_identity(&transaction, identity)?;
        return transaction.commit().map_err(sql_error);
    }

    transaction
        .execute_batch(
            "
            CREATE TABLE registry_meta (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              schema_version INTEGER NOT NULL,
              project_id TEXT NOT NULL,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              runtime_abi TEXT NOT NULL,
              toolchain_id TEXT NOT NULL,
              created_at TEXT NOT NULL
            );

            CREATE TABLE events (
              seq INTEGER PRIMARY KEY,
              at TEXT NOT NULL,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              event_type TEXT NOT NULL,
              run_id TEXT,
              service_instance_id TEXT,
              process_key TEXT,
              computed_manifest_hash TEXT,
              payload_json TEXT NOT NULL
            );

            CREATE TABLE runs (
              run_id TEXT PRIMARY KEY,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              execution_outcome TEXT CHECK (execution_outcome IN ('succeeded', 'failed', 'canceled', 'interrupted')),
              finalization TEXT NOT NULL DEFAULT 'unfinished' CHECK (finalization IN ('unfinished', 'complete')),
              manifest_path TEXT NOT NULL,
              computed_manifest_hash TEXT NOT NULL,
              runtime_abi TEXT NOT NULL,
              toolchain_id TEXT NOT NULL,
              generator_json TEXT NOT NULL,
              target_json TEXT NOT NULL,
              source_json TEXT NOT NULL,
              summary_path TEXT,
              CHECK (finalization != 'complete' OR execution_outcome IS NOT NULL)
            );

            CREATE TABLE processes (
              process_key TEXT PRIMARY KEY,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              pid INTEGER NOT NULL,
              pgid INTEGER NOT NULL,
              start_identity TEXT NOT NULL,
              command_json TEXT NOT NULL,
              run_id TEXT NOT NULL,
              service_instance_id TEXT,
              service_name TEXT,
              execution_outcome TEXT CHECK (execution_outcome IN ('succeeded', 'failed', 'canceled', 'interrupted')),
              exit_code INTEGER,
              status TEXT NOT NULL,
              CHECK ((service_instance_id IS NULL) = (service_name IS NULL)),
              CHECK (service_name IS NULL OR length(service_name) > 0),
              CHECK (execution_outcome IS NOT NULL OR exit_code IS NULL),
              CHECK (execution_outcome != 'succeeded' OR exit_code IS NOT NULL)
            );

            CREATE TABLE ports (
              endpoint_key TEXT PRIMARY KEY,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              service_instance_id TEXT NOT NULL,
              address TEXT NOT NULL,
              port INTEGER NOT NULL,
              status TEXT NOT NULL,
              owner_process_key TEXT NOT NULL
            );

            CREATE TABLE cleanups (
              cleanup_id TEXT PRIMARY KEY,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              target_path TEXT NOT NULL,
              purge INTEGER NOT NULL CHECK (purge IN (0, 1)),
              marker_json TEXT,
              status TEXT NOT NULL,
              refusal_reason TEXT
            );
            ",
        )
        .map_err(sql_error)?;

    transaction
        .execute(
            &format!(
                "
            INSERT INTO registry_meta (
              id, schema_version, project_id, environment, slot,
              runtime_abi, toolchain_id, created_at
            ) VALUES (1, {SCHEMA_VERSION}, ?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            "
            ),
            (
                &identity.project_id,
                &identity.environment,
                identity.slot,
                &identity.runtime_abi,
                &identity.toolchain_id,
            ),
        )
        .map_err(sql_error)?;

    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(sql_error)?;
    transaction.commit().map_err(sql_error)
}

fn verify_required_columns(conn: &Connection) -> RuntimeResult<()> {
    for (table, columns) in [
        ("events", &["environment", "slot"][..]),
        (
            "runs",
            &["environment", "slot", "execution_outcome", "finalization"][..],
        ),
        (
            "processes",
            &[
                "environment",
                "slot",
                "execution_outcome",
                "exit_code",
                "service_name",
            ][..],
        ),
        ("ports", &["environment", "slot"][..]),
        ("cleanups", &["environment", "slot", "purge"][..]),
    ] {
        let mut statement = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(sql_error)?;
        let found = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(sql_error)?
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .map_err(sql_error)?;
        for column in columns {
            if !found.contains(*column) {
                return Err(RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    format!("registry table {table} is missing required column {column}"),
                ));
            }
        }
    }
    Ok(())
}

fn verify_identity(conn: &Connection, identity: &RegistryIdentity) -> RuntimeResult<()> {
    let existing = conn
        .query_row(
            "SELECT schema_version, project_id, environment, slot, runtime_abi, toolchain_id FROM registry_meta WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()
        .map_err(sql_error)?;
    let Some((schema_version, project_id, environment, slot, runtime_abi, toolchain_id)) = existing
    else {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "registry metadata row is missing",
        ));
    };
    if schema_version != SCHEMA_VERSION {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("expected registry schema_version {SCHEMA_VERSION}, got {schema_version}"),
        ));
    }

    let found = RegistryIdentity {
        project_id,
        environment,
        slot,
        runtime_abi,
        toolchain_id,
    };
    let mut mismatched_fields = Vec::new();
    if found.project_id != identity.project_id {
        mismatched_fields.push("projectId");
    }
    if found.environment != identity.environment {
        mismatched_fields.push("environment");
    }
    if found.slot != identity.slot {
        mismatched_fields.push("slot");
    }
    if found.runtime_abi != identity.runtime_abi {
        mismatched_fields.push("runtimeAbi");
    }
    if found.toolchain_id != identity.toolchain_id {
        mismatched_fields.push("toolchainId");
    }
    if !mismatched_fields.is_empty() {
        let ownership_mismatch = mismatched_fields
            .iter()
            .any(|field| matches!(*field, "projectId" | "environment" | "slot"));
        let (code, message) = if ownership_mismatch {
            (
                ErrorCode::StateUnowned,
                "registry metadata is owned by a different project/environment/slot identity",
            )
        } else {
            (
                ErrorCode::RuntimeAbiMismatch,
                "registry metadata was written under a different runtime ABI/toolchain identity",
            )
        };
        return Err(RuntimeError::new(code, message)
            .with_detail("expectedRegistryIdentity", registry_identity_json(identity))
            .with_detail("foundRegistryIdentity", registry_identity_json(&found))
            .with_detail("mismatchedFields", mismatched_fields));
    }
    Ok(())
}

fn registry_identity_json(identity: &RegistryIdentity) -> Value {
    json!(RegistryIdentityDiagnostic {
        project_id: &identity.project_id,
        environment: &identity.environment,
        slot: identity.slot,
        runtime_abi: &identity.runtime_abi,
        toolchain_id: &identity.toolchain_id,
    })
}

fn user_table_count(conn: &Connection) -> RuntimeResult<i64> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map_err(sql_error)
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string())
}

/// Validate an existing registry without repairing, initializing or changing its
/// journal settings. The caller holds a coherent SQLite read transaction.
pub(crate) fn verify_existing(conn: &Connection, identity: &RegistryIdentity) -> RuntimeResult<()> {
    let version = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(sql_error)?;
    if version != SCHEMA_VERSION {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("expected registry user_version {SCHEMA_VERSION}, got {version}"),
        ));
    }
    verify_required_columns(conn)?;
    verify_identity(conn, identity)
}
