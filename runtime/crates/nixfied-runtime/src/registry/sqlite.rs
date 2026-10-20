use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::filesystem::FileIdentity;
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
            redactor: &self.redactor,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn identity(&self) -> &RegistryIdentity {
        &self.identity
    }

    pub fn set_redactor(&mut self, redactor: Redactor) {
        self.redactor = redactor;
    }

    pub fn append_event(&mut self, event: EventInsert<'_>) -> RuntimeResult<i64> {
        self.guard.validate()?;
        append_event(&mut self.conn, &self.redactor, event)
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
        match read_existing(path, identity, std::time::Duration::from_secs(5))? {
            RegistryRead::Absent => Ok(None),
            RegistryRead::Snapshot { reader, .. } => Ok(Some(reader)),
            RegistryRead::BootstrapEmpty(_) => Err(with_registry_path(
                RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    "registry schema is not initialized",
                ),
                path,
            )),
            RegistryRead::Busy(error) => Err(with_registry_path(super::sql_error(error), path)),
        }
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// Native read classification shared by strict controls and polling observers.
/// Only a complete snapshot owns a reader; bootstrap and busy retain no read
/// transaction that could conceal subsequent schema publication.
pub(crate) enum RegistryRead {
    Absent,
    BootstrapEmpty(FileIdentity),
    Busy(rusqlite::Error),
    Snapshot {
        reader: RegistryReader,
        file: FileIdentity,
    },
}

impl RegistryRead {
    fn file_identity(&self) -> Option<FileIdentity> {
        match self {
            Self::BootstrapEmpty(file) | Self::Snapshot { file, .. } => Some(*file),
            Self::Absent | Self::Busy(_) => None,
        }
    }
}

pub(crate) fn read_existing(
    path: &Path,
    identity: &RegistryIdentity,
    busy_timeout: std::time::Duration,
) -> RuntimeResult<RegistryRead> {
    match read_attempt(path, identity, busy_timeout) {
        Ok(read) => Ok(read),
        Err(schema::ReadFailure::Sql(error)) if is_contention(&error) => {
            Ok(RegistryRead::Busy(error))
        }
        Err(error) => Err(with_registry_path(error.into(), path)),
    }
}

fn read_attempt(
    path: &Path,
    identity: &RegistryIdentity,
    busy_timeout: std::time::Duration,
) -> Result<RegistryRead, schema::ReadFailure> {
    let Some(file) = registry_file_identity(path)? else {
        return Ok(RegistryRead::Absent);
    };
    // NOFOLLOW rejects any symlink component on macOS. System aliases such as
    // /tmp and /var are legitimate ancestors; resolve only the parent.
    let parent = path
        .parent()
        .ok_or_else(|| invalid_read("registry path has no parent"))?;
    let directory =
        std::fs::canonicalize(parent).map_err(|error| invalid_read(error.to_string()))?;
    let filename = path
        .file_name()
        .ok_or_else(|| invalid_read("registry path has no filename"))?;
    let conn = Connection::open_with_flags(
        directory.join(filename),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.busy_timeout(busy_timeout)?;
    conn.execute_batch("BEGIN")?;
    let read = if schema::is_empty_unversioned(&conn)? {
        RegistryRead::BootstrapEmpty(file)
    } else {
        schema::verify_existing(&conn, identity)?;
        RegistryRead::Snapshot {
            reader: RegistryReader { conn },
            file,
        }
    };
    // The checked stamp brackets actual reader opening and snapshot validation;
    // never attach a previous file's history to replacement bytes.
    if registry_file_identity(path)? != read.file_identity() {
        return Err(invalid_read("registry file changed while opening its snapshot").into());
    }
    Ok(read)
}

fn registry_file_identity(path: &Path) -> RuntimeResult<Option<FileIdentity>> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(invalid_read(error.to_string())),
        Ok(metadata) if !metadata.is_file() => Err(invalid_read("registry is not a regular file")),
        Ok(metadata) => Ok(Some(FileIdentity::from_metadata(&metadata))),
    }
}

fn invalid_read(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, message)
}

fn is_contention(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::SqliteFailure(error, _)
        if matches!(error.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn identity() -> RegistryIdentity {
        RegistryIdentity::for_slot("project", "dev", 0, "abi", "toolchain")
    }

    #[test]
    fn polling_distinguishes_absence_exact_bootstrap_and_corruption_without_writes() {
        let root = crate::test_support::TestDir::new("registry-read-classifier");
        let missing = root.join("missing/registry.sqlite3");
        assert!(matches!(
            read_existing(&missing, &identity(), Duration::ZERO).unwrap(),
            RegistryRead::Absent
        ));
        assert!(!root.join("missing").exists());
        let path = root.join("registry.sqlite3");
        let writer = Connection::open(&path).unwrap();
        assert!(matches!(
            read_existing(&path, &identity(), Duration::ZERO).unwrap(),
            RegistryRead::BootstrapEmpty(_)
        ));
        assert_eq!(
            RegistryReader::open_existing(&path, &identity())
                .err()
                .unwrap()
                .code,
            ErrorCode::RegistryCorrupt
        );
        assert_eq!(
            writer
                .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        writer
            .execute_batch("CREATE TABLE unexpected (value TEXT)")
            .unwrap();
        assert_eq!(
            read_existing(&path, &identity(), Duration::ZERO)
                .err()
                .unwrap()
                .code,
            ErrorCode::RegistryCorrupt
        );
        drop(writer);
        std::fs::write(&path, b"this is not SQLite registry data").unwrap();
        assert_eq!(
            read_existing(&path, &identity(), Duration::ZERO)
                .err()
                .unwrap()
                .code,
            ErrorCode::RegistryCorrupt
        );
    }

    #[test]
    fn exclusive_sqlite_contention_is_transient_but_unlocked_invalid_schema_is_not() {
        let root = crate::test_support::TestDir::new("registry-busy");
        let path = root.join("registry.sqlite3");
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch("CREATE TABLE unexpected (value TEXT); BEGIN EXCLUSIVE")
            .unwrap();
        assert!(matches!(
            read_existing(&path, &identity(), Duration::ZERO).unwrap(),
            RegistryRead::Busy(_)
        ));
        writer.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            read_existing(&path, &identity(), Duration::ZERO)
                .err()
                .unwrap()
                .code,
            ErrorCode::RegistryCorrupt
        );
        // Extended contention codes preserve their primary category. I/O and
        // malformed database codes never become retry credit.
        for code in [
            rusqlite::ffi::SQLITE_BUSY,
            rusqlite::ffi::SQLITE_BUSY_RECOVERY,
            rusqlite::ffi::SQLITE_BUSY_SNAPSHOT,
            rusqlite::ffi::SQLITE_LOCKED,
            rusqlite::ffi::SQLITE_LOCKED_SHAREDCACHE,
        ] {
            assert!(is_contention(&rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(code),
                None
            )));
        }
        for code in [
            rusqlite::ffi::SQLITE_IOERR,
            rusqlite::ffi::SQLITE_NOTADB,
            rusqlite::ffi::SQLITE_CORRUPT,
            rusqlite::ffi::SQLITE_CANTOPEN,
        ] {
            assert!(!is_contention(&rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(code),
                None
            )));
        }
    }

    #[test]
    fn schema_publication_and_progress_require_fresh_read_transactions() {
        let root = crate::test_support::TestDir::new("registry-publication");
        let path = root.join("registry.sqlite3");
        let mut writer = Connection::open(&path).unwrap();
        assert!(matches!(
            read_existing(&path, &identity(), Duration::ZERO).unwrap(),
            RegistryRead::BootstrapEmpty(_)
        ));
        schema::initialize(&mut writer, &identity()).unwrap();
        let RegistryRead::Snapshot { reader, .. } =
            read_existing(&path, &identity(), Duration::ZERO).unwrap()
        else {
            panic!("schema did not publish")
        };
        let maximum = |reader: &RegistryReader| {
            reader
                .connection()
                .query_row("SELECT coalesce(max(seq), 0) FROM events", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap()
        };
        assert_eq!(maximum(&reader), 0);
        writer
            .execute(
                "INSERT INTO events(at, event_type, payload_json) VALUES ('now', 'test', '{}')",
                [],
            )
            .unwrap();
        assert_eq!(
            maximum(&reader),
            0,
            "a retained snapshot cannot observe later publication"
        );
        drop(reader);
        let RegistryRead::Snapshot { reader, .. } =
            read_existing(&path, &identity(), Duration::ZERO).unwrap()
        else {
            panic!("complete registry was lost")
        };
        assert_eq!(maximum(&reader), 1);
    }
}
