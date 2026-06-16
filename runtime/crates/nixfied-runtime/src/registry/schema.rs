use rusqlite::{Connection, OptionalExtension};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::records::RegistryIdentity;

pub const SCHEMA_VERSION: i64 = 5;

pub fn initialize(conn: &mut Connection, identity: &RegistryIdentity) -> RuntimeResult<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        -- A run's heartbeat thread opens its own connection and writes the lease
        -- concurrently with the main thread. WAL still serializes writers, so
        -- without a busy timeout a transient collision returns SQLITE_BUSY, which
        -- the runtime maps to REGISTRY_CORRUPT and would fail an otherwise healthy
        -- run. Wait instead of erroring.
        PRAGMA busy_timeout = 5000;
        ",
    )
    .map_err(sql_error)?;
    let starting_user_version = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(sql_error)?;
    if starting_user_version != 0 && starting_user_version != SCHEMA_VERSION {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("expected registry user_version {SCHEMA_VERSION}, got {starting_user_version}"),
        ));
    }
    let sqlite_table_count = user_table_count(conn)?;
    let is_new_registry = starting_user_version == 0 && sqlite_table_count == 0;
    if !is_new_registry {
        verify_existing(conn)?;
        verify_identity(conn, identity)?;
        return Ok(());
    }

    let transaction = conn.transaction().map_err(sql_error)?;
    transaction
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS registry_meta (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              schema_version INTEGER NOT NULL,
              project_id TEXT NOT NULL,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              runtime_abi TEXT NOT NULL,
              toolchain_id TEXT NOT NULL,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS events (
              seq INTEGER PRIMARY KEY,
              at TEXT NOT NULL,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              event_type TEXT NOT NULL,
              run_id TEXT,
              service_instance_id TEXT,
              process_key TEXT,
              computed_model_hash TEXT,
              payload_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS runs (
              run_id TEXT PRIMARY KEY,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              status TEXT NOT NULL,
              model_path TEXT NOT NULL,
              computed_model_hash TEXT NOT NULL,
              runtime_abi TEXT NOT NULL,
              toolchain_id TEXT NOT NULL,
              generator_json TEXT NOT NULL,
              target_json TEXT NOT NULL,
              source_json TEXT NOT NULL,
              summary_path TEXT
            );

            CREATE TABLE IF NOT EXISTS services (
              service_instance_id TEXT PRIMARY KEY,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              service_name TEXT NOT NULL,
              service_address_hash TEXT NOT NULL,
              endpoint_identity_hash TEXT NOT NULL,
              state_identity_hash TEXT NOT NULL,
              runtime_compatibility_hash TEXT NOT NULL,
              target_identity_hash TEXT NOT NULL,
              service_lifetime TEXT NOT NULL,
              status TEXT NOT NULL,
              endpoint_json TEXT NOT NULL,
              state_root TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS processes (
              process_key TEXT PRIMARY KEY,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              pid INTEGER NOT NULL,
              pgid INTEGER NOT NULL,
              start_identity TEXT NOT NULL,
              command_json TEXT NOT NULL,
              run_id TEXT NOT NULL,
              service_instance_id TEXT,
              status TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS ports (
              endpoint_key TEXT PRIMARY KEY,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              service_instance_id TEXT NOT NULL,
              address TEXT NOT NULL,
              port INTEGER NOT NULL,
              status TEXT NOT NULL,
              owner_process_key TEXT
            );

            -- One lease row per (run, service): a multi-service run reserves each
            -- service independently, so each carries its own cross-run lease.
            CREATE TABLE IF NOT EXISTS run_leases (
              run_id TEXT NOT NULL,
              environment TEXT NOT NULL,
              slot INTEGER NOT NULL CHECK (slot >= 0),
              service_instance_id TEXT NOT NULL,
              owner_token TEXT NOT NULL,
              heartbeat_at TEXT NOT NULL,
              expires_at TEXT NOT NULL,
              status TEXT NOT NULL,
              PRIMARY KEY (run_id, service_instance_id)
            );

            CREATE TABLE IF NOT EXISTS cleanups (
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

    transaction.commit().map_err(sql_error)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(sql_error)?;
    let user_version = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(sql_error)?;
    if user_version != SCHEMA_VERSION {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("expected registry user_version {SCHEMA_VERSION}, got {user_version}"),
        ));
    }
    verify_identity(conn, identity)?;
    Ok(())
}

pub fn verify_existing(conn: &Connection) -> RuntimeResult<()> {
    let user_version = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(sql_error)?;
    if user_version != SCHEMA_VERSION {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("expected registry user_version {SCHEMA_VERSION}, got {user_version}"),
        ));
    }
    verify_required_columns(conn)?;
    Ok(())
}

fn verify_required_columns(conn: &Connection) -> RuntimeResult<()> {
    for (table, columns) in [
        ("events", &["environment", "slot"][..]),
        ("runs", &["environment", "slot"][..]),
        ("services", &["environment", "slot", "service_lifetime"][..]),
        ("processes", &["environment", "slot"][..]),
        ("ports", &["environment", "slot"][..]),
        (
            "run_leases",
            &["environment", "slot", "service_instance_id", "status"][..],
        ),
        ("cleanups", &["environment", "slot", "purge"][..]),
    ] {
        for column in columns {
            if !has_column(conn, table, column)? {
                return Err(RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    format!("registry table {table} is missing required column {column}"),
                ));
            }
        }
    }
    Ok(())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> RuntimeResult<bool> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(sql_error)?;
    let mut rows = statement.query([]).map_err(sql_error)?;
    while let Some(row) = rows.next().map_err(sql_error)? {
        let name = row.get::<_, String>(1).map_err(sql_error)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
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
    if schema_version != SCHEMA_VERSION
        || project_id != identity.project_id
        || environment != identity.environment
        || slot != identity.slot
        || runtime_abi != identity.runtime_abi
        || toolchain_id != identity.toolchain_id
    {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "registry metadata does not match the selected identity",
        ));
    }
    Ok(())
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
