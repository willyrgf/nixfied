use rusqlite::{Connection, params};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventInsert {
    pub event_type: String,
    pub run_id: Option<String>,
    pub service_instance_id: Option<String>,
    pub process_key: Option<String>,
    pub computed_model_hash: Option<String>,
    pub payload_json: String,
}

impl EventInsert {
    pub fn new(event_type: impl Into<String>, payload_json: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            run_id: None,
            service_instance_id: None,
            process_key: None,
            computed_model_hash: None,
            payload_json: payload_json.into(),
        }
    }
}

pub fn append_event(conn: &mut Connection, event: &EventInsert) -> RuntimeResult<i64> {
    let transaction = conn
        .transaction()
        .map_err(|error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()))?;
    transaction
        .execute(
            "
            INSERT INTO events (
              at, event_type, run_id, service_instance_id, process_key,
              computed_model_hash, payload_json
            ) VALUES (
              strftime('%Y-%m-%dT%H:%M:%fZ','now'), ?1, ?2, ?3, ?4, ?5, ?6
            )
            ",
            params![
                event.event_type,
                event.run_id,
                event.service_instance_id,
                event.process_key,
                event.computed_model_hash,
                event.payload_json,
            ],
        )
        .map_err(|error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()))?;
    let seq = transaction.last_insert_rowid();
    transaction
        .commit()
        .map_err(|error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()))?;
    Ok(seq)
}
