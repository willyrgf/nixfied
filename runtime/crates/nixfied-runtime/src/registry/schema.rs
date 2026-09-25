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

pub const SCHEMA_VERSION: i64 = 15;

const SCHEMA_SQL: &str = "
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
              role TEXT NOT NULL CHECK (role IN ('task', 'service', 'probe')),
              execution_outcome TEXT CHECK (execution_outcome IN ('succeeded', 'failed', 'canceled', 'interrupted')),
              exit_code INTEGER,
              status TEXT NOT NULL,
              CHECK ((role = 'service') = (service_instance_id IS NOT NULL)),
              CHECK ((role = 'task') = (service_name IS NULL)),
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
              target TEXT NOT NULL,
              data_generation TEXT NOT NULL UNIQUE CHECK (length(data_generation) > 0),
              marker_json TEXT NOT NULL,
              purge INTEGER NOT NULL CHECK (purge IN (0, 1)),
              root_identity TEXT NOT NULL,
              status TEXT NOT NULL CHECK (status IN ('pending', 'completed'))
            );

            CREATE UNIQUE INDEX cleanups_one_pending ON cleanups (status) WHERE status = 'pending';
            ";

pub fn initialize(conn: &mut Connection, identity: &RegistryIdentity) -> RuntimeResult<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(sql_error)?;
    // Existing bytes are admitted in a read snapshot before journal conversion
    // or any schema write. An empty unversioned database is the only bootstrap.
    {
        let snapshot = conn.transaction().map_err(sql_error)?;
        if !is_empty_unversioned(&snapshot)? {
            verify_existing(&snapshot, identity)?;
        }
        snapshot.commit().map_err(sql_error)?;
    }
    configure_durability(conn)?;
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    if !is_empty_unversioned(&transaction)? {
        verify_existing(&transaction, identity)?;
        return transaction.commit().map_err(sql_error);
    }

    transaction.execute_batch(SCHEMA_SQL).map_err(sql_error)?;

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

fn configure_durability(conn: &Connection) -> RuntimeResult<()> {
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(sql_error)?;
    if mode != "wal" {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "registry requires WAL journal mode",
        ));
    }
    conn.execute_batch("PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON;")
        .map_err(sql_error)?;
    #[cfg(target_os = "macos")]
    conn.execute_batch("PRAGMA fullfsync = ON; PRAGMA checkpoint_fullfsync = ON;")
        .map_err(sql_error)?;
    for (setting, required) in [("synchronous", 2), ("foreign_keys", 1)] {
        verify_setting(conn, setting, required)?;
    }
    #[cfg(target_os = "macos")]
    for setting in ["fullfsync", "checkpoint_fullfsync"] {
        verify_setting(conn, setting, 1)?;
    }
    Ok(())
}

fn verify_setting(conn: &Connection, setting: &str, required: i64) -> RuntimeResult<()> {
    let actual: i64 = conn
        .query_row(&format!("PRAGMA {setting}"), [], |row| row.get(0))
        .map_err(sql_error)?;
    if actual != required {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "registry durability setting was not established",
        ));
    }
    Ok(())
}

fn is_empty_unversioned(conn: &Connection) -> RuntimeResult<bool> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(sql_error)?;
    let count: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))
        .map_err(sql_error)?;
    Ok(version == 0 && count == 0)
}

#[derive(PartialEq, Eq)]
struct SchemaObject {
    kind: String,
    name: String,
    table: String,
    sql: Option<String>,
}

fn schema_objects(conn: &Connection) -> RuntimeResult<Vec<SchemaObject>> {
    conn.prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type, name")
        .map_err(sql_error)?
        .query_map([], |row| {
            Ok(SchemaObject {
                kind: row.get(0)?,
                name: row.get(1)?,
                table: row.get(2)?,
                sql: row.get(3)?,
            })
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)
}

fn verify_schema(conn: &Connection) -> RuntimeResult<()> {
    // Let SQLite interpret the one authored definition, including constraints
    // and implicit indexes. Do not maintain a second partial column inventory.
    let expected = Connection::open_in_memory().map_err(sql_error)?;
    expected.execute_batch(SCHEMA_SQL).map_err(sql_error)?;
    if schema_objects(conn)? != schema_objects(&expected)? {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "registry schema does not match its exact version",
        ));
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
    verify_schema(conn)?;
    verify_identity(conn, identity)
}
