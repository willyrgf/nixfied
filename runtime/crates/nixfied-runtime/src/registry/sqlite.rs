use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::redaction::Redactor;
use crate::registry::events::{EventInsert, append_event};
use crate::registry::records::RegistryIdentity;
use crate::registry::schema;
use crate::state::ownership::SlotGuard;

pub struct Registry {
    path: PathBuf,
    conn: Connection,
    identity: RegistryIdentity,
    redactor: Redactor,
    // SQLite drops before the guard on unwind.
    guard: SlotGuard,
}

pub(crate) struct RegistryContext<'a> {
    pub(crate) connection: &'a mut Connection,
    pub(crate) identity: &'a RegistryIdentity,
    pub(crate) redactor: &'a Redactor,
}

impl Registry {
    pub fn open_or_create(guard: SlotGuard, identity: &RegistryIdentity) -> RuntimeResult<Self> {
        guard.validate()?;
        guard.check_identity(&identity.project_id, &identity.environment, identity.slot)?;
        let path = guard.registry_path().to_path_buf();
        let mut conn = Connection::open(&path).map_err(|error| {
            with_registry_path(
                RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()),
                &path,
            )
        })?;
        schema::initialize(&mut conn, identity)
            .map_err(|error| with_registry_path(error, &path))?;
        Ok(Self {
            path,
            conn,
            identity: identity.clone(),
            redactor: Redactor::empty(),
            guard,
        })
    }

    pub fn authority(&self) -> &SlotGuard {
        &self.guard
    }

    /// End database access before releasing slot authority. Preserve both close
    /// failures without retrying a potentially reused raw descriptor.
    pub fn close(self) -> RuntimeResult<()> {
        let Self {
            conn, guard, path, ..
        } = self;
        let database = conn.close().map_err(|(connection, error)| {
            drop(connection);
            with_registry_path(
                RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    format!("failed to close registry: {error}"),
                ),
                &path,
            )
        });
        crate::error::both(database, guard.release())
    }

    pub(crate) fn context(&mut self) -> RuntimeResult<RegistryContext<'_>> {
        self.guard.validate()?;
        Ok(RegistryContext {
            connection: &mut self.conn,
            identity: &self.identity,
            redactor: &self.redactor,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn identity(&self) -> &RegistryIdentity {
        &self.identity
    }

    pub fn set_redactor(&mut self, redactor: Redactor) {
        self.redactor = redactor;
    }

    pub fn append_event(&mut self, event: EventInsert<'_>) -> RuntimeResult<i64> {
        self.guard.validate()?;
        append_event(&mut self.conn, &self.identity, &self.redactor, event)
    }
}

fn with_registry_path(error: RuntimeError, path: &Path) -> RuntimeError {
    let error = error.with_detail("registryPath", path);
    if let Some(parent) = path.parent() {
        error.with_detail("registryDir", parent)
    } else {
        error
    }
}

/// A coherent database snapshot with no slot mutation authority. Opening an
/// absent registry returns absence and never bootstraps its parent directories.
pub struct RegistryReader {
    conn: Connection,
}

impl RegistryReader {
    pub fn open_existing(path: &Path, identity: &RegistryIdentity) -> RuntimeResult<Option<Self>> {
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(with_registry_path(
                    RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()),
                    path,
                ));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(with_registry_path(
                    RuntimeError::new(ErrorCode::RegistryCorrupt, "registry is not a regular file"),
                    path,
                ));
            }
            Ok(_) => {}
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|error| {
            with_registry_path(
                RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()),
                path,
            )
        })?;
        conn.execute_batch("BEGIN")
            .map_err(|error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()))?;
        schema::verify_existing(&conn, identity)
            .map_err(|error| with_registry_path(error, path))?;
        Ok(Some(Self { conn }))
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }
}
