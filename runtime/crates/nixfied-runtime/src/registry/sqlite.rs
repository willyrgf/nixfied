use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::redaction::Redactor;
use crate::registry::events::{EventInsert, append_event};
use crate::registry::records::RegistryIdentity;
use crate::registry::schema;

pub struct Registry {
    path: PathBuf,
    conn: Connection,
    identity: RegistryIdentity,
    redactor: Redactor,
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
        Ok(Self {
            path,
            conn,
            identity: identity.clone(),
            redactor: Redactor::empty(),
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

    pub fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    pub fn redact_payload_json(&self, payload_json: &str) -> RuntimeResult<String> {
        self.redactor.redact_json_str(payload_json)
    }

    pub fn append_event(&mut self, event: &EventInsert) -> RuntimeResult<i64> {
        let mut event = event.clone();
        event.payload_json = self.redact_payload_json(&event.payload_json)?;
        append_event(&mut self.conn, &self.identity, &event)
    }
}
