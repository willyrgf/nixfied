use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::events::{EventInsert, append_event};
use crate::registry::records::RegistryIdentity;
use crate::registry::schema;

pub struct Registry {
    path: PathBuf,
    conn: Connection,
}

impl Registry {
    pub fn open_or_create(
        path: impl AsRef<Path>,
        identity: &RegistryIdentity,
    ) -> RuntimeResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    format!(
                        "failed to create registry dir {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
        let mut conn = Connection::open(&path)
            .map_err(|error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()))?;
        schema::initialize(&mut conn, identity)?;
        Ok(Self { path, conn })
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

    pub fn append_event(&mut self, event: &EventInsert) -> RuntimeResult<i64> {
        append_event(&mut self.conn, event)
    }
}
