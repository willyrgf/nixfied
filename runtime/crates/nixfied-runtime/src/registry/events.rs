use rusqlite::{Connection, Transaction, params};

use crate::error::RuntimeResult;
use crate::redaction::Redactor;
use crate::registry::RegistryIdentity;

pub struct EventInsert<'a> {
    pub event_type: &'a str,
    pub run_id: Option<&'a str>,
    pub service_instance_id: Option<&'a str>,
    pub process_key: Option<&'a str>,
    pub computed_manifest_hash: Option<&'a str>,
    pub payload_json: &'a str,
}

impl<'a> EventInsert<'a> {
    pub fn new(event_type: &'a str, payload_json: &'a str) -> Self {
        Self {
            event_type,
            run_id: None,
            service_instance_id: None,
            process_key: None,
            computed_manifest_hash: None,
            payload_json,
        }
    }
}

pub(crate) fn append_event(
    conn: &mut Connection,
    identity: &RegistryIdentity,
    redactor: &Redactor,
    event: EventInsert<'_>,
) -> RuntimeResult<i64> {
    let transaction = conn.transaction().map_err(super::sql_error)?;
    let seq = insert_event(&transaction, identity, redactor, event)?;
    transaction.commit().map_err(super::sql_error)?;
    Ok(seq)
}

pub(crate) fn insert_event(
    transaction: &Transaction<'_>,
    identity: &RegistryIdentity,
    redactor: &Redactor,
    event: EventInsert<'_>,
) -> RuntimeResult<i64> {
    let payload_json = redactor.redact_json_str(event.payload_json)?;
    transaction
        .execute(
            "
            INSERT INTO events (
              at, environment, slot, event_type, run_id, service_instance_id,
              process_key, computed_manifest_hash, payload_json
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
                event.computed_manifest_hash,
                payload_json,
            ],
        )
        .map_err(super::sql_error)?;
    Ok(transaction.last_insert_rowid())
}
