use rusqlite::{Connection, Transaction, params};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::redaction::Redactor;
use crate::registry::RegistryIdentity;

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

pub(crate) struct BorrowedEvent<'a> {
    pub(crate) event_type: &'a str,
    pub(crate) run_id: Option<&'a str>,
    pub(crate) service_instance_id: Option<&'a str>,
    pub(crate) process_key: Option<&'a str>,
    pub(crate) computed_model_hash: Option<&'a str>,
    pub(crate) payload_json: &'a str,
}

pub(crate) fn append_event(
    conn: &mut Connection,
    identity: &RegistryIdentity,
    redactor: &Redactor,
    event: &EventInsert,
) -> RuntimeResult<i64> {
    let transaction = conn
        .transaction()
        .map_err(|error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()))?;
    let seq = insert_event(
        &transaction,
        identity,
        redactor,
        BorrowedEvent {
            event_type: &event.event_type,
            run_id: event.run_id.as_deref(),
            service_instance_id: event.service_instance_id.as_deref(),
            process_key: event.process_key.as_deref(),
            computed_model_hash: event.computed_model_hash.as_deref(),
            payload_json: &event.payload_json,
        },
    )?;
    transaction
        .commit()
        .map_err(|error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()))?;
    Ok(seq)
}

pub(crate) fn insert_event(
    transaction: &Transaction<'_>,
    identity: &RegistryIdentity,
    redactor: &Redactor,
    event: BorrowedEvent<'_>,
) -> RuntimeResult<i64> {
    let payload_json = redactor.redact_json_str(event.payload_json)?;
    transaction
        .execute(
            "
            INSERT INTO events (
              at, environment, slot, event_type, run_id, service_instance_id,
              process_key, computed_model_hash, payload_json
            ) VALUES (
              strftime('%Y-%m-%dT%H:%M:%fZ','now'), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
            )
            ",
            params![
                identity.environment,
                identity.slot,
                event.event_type,
                event.run_id,
                event.service_instance_id,
                event.process_key,
                event.computed_model_hash,
                payload_json,
            ],
        )
        .map_err(|error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string()))?;
    Ok(transaction.last_insert_rowid())
}
