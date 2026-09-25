use rusqlite::{Connection, Transaction, params};

use crate::error::RuntimeResult;
use crate::redaction::Redactor;

/// One event in the slot's total order. The registry is per slot, so the
/// event names only the run and process it concerns.
pub struct EventInsert<'a> {
    pub event_type: &'a str,
    pub run_id: Option<&'a str>,
    pub process_key: Option<&'a str>,
    pub payload_json: &'a str,
}

impl<'a> EventInsert<'a> {
    pub fn new(event_type: &'a str, payload_json: &'a str) -> Self {
        Self {
            event_type,
            run_id: None,
            process_key: None,
            payload_json,
        }
    }
}

pub(crate) fn append_event(
    conn: &mut Connection,
    redactor: &Redactor,
    event: EventInsert<'_>,
) -> RuntimeResult<i64> {
    let transaction = conn.transaction().map_err(super::sql_error)?;
    let seq = insert_event(&transaction, redactor, event)?;
    transaction.commit().map_err(super::sql_error)?;
    Ok(seq)
}

pub(crate) fn insert_event(
    transaction: &Transaction<'_>,
    redactor: &Redactor,
    event: EventInsert<'_>,
) -> RuntimeResult<i64> {
    let payload_json = redactor.redact_json_str(event.payload_json)?;
    transaction
        .execute(
            "
            INSERT INTO events (at, event_type, run_id, process_key, payload_json)
            VALUES (strftime('%Y-%m-%dT%H:%M:%fZ','now'), ?1, ?2, ?3, ?4)
            ",
            params![
                event.event_type,
                event.run_id,
                event.process_key,
                payload_json,
            ],
        )
        .map_err(super::sql_error)?;
    Ok(transaction.last_insert_rowid())
}
